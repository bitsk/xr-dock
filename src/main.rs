// SPDX-License-Identifier: GPL-3.0-or-later
mod catalog;
mod clock;
mod model;
mod tasks;

use anyhow::{Context, Result, bail};
use model::{Intent, Model};
use serde_json::json;
use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use xr_shell_protocols::Client;

struct Options {
    socket: PathBuf,
    seconds: u64,
    app_dirs: Vec<PathBuf>,
    pins: Vec<String>,
    list: bool,
    list_pins: bool,
}
fn options() -> Result<Option<Options>> {
    let runtime = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let mut o = Options {
        socket: runtime
            .unwrap_or_else(|| PathBuf::from("/run/user").join(uid_directory()))
            .join("xr-desktop/shell.sock"),
        seconds: 0,
        app_dirs: Vec::new(),
        pins: Vec::new(),
        list: false,
        list_pins: false,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => o.socket = args.next().context("--socket requires a path")?.into(),
            "--seconds" => {
                o.seconds = args
                    .next()
                    .context("--seconds requires an integer")?
                    .parse()?
            }
            "--apps-dir" => o
                .app_dirs
                .push(args.next().context("--apps-dir requires a path")?.into()),
            "--pin" => o
                .pins
                .push(args.next().context("--pin requires a desktop ID")?),
            "--list-apps" => o.list = true,
            "--list-pins" => o.list_pins = true,
            "--help" | "-h" => {
                println!(
                    "xr-dock [--socket PATH] [--seconds N] [--apps-dir DIR ...] [--pin APP.desktop ...] [--list-apps|--list-pins]\n\
                    Uses the Shell scene SDK; task/workspace state comes only from its snapshots.\n\
                    A custom --apps-dir replaces the normal XDG application directory list."
                );
                return Ok(None);
            }
            _ => bail!("unknown option: {arg}"),
        }
    }
    if o.app_dirs.is_empty() {
        o.app_dirs = catalog::directories();
    }
    Ok(Some(o))
}
fn uid_directory() -> String {
    // /proc identifies this process; no account-name or hardware identity is baked in.
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(2))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".into())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("XR_DOCK_ERROR={error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let Some(options) = options()? else {
        return Ok(());
    };
    let apps = catalog::load(&options.app_dirs);
    if options.list {
        println!("{}", serde_json::to_string_pretty(&apps)?);
        return Ok(());
    }
    let pins = catalog::pinned(&apps, &options.pins);
    if options.list_pins {
        println!("{}", serde_json::to_string_pretty(&pins)?);
        return Ok(());
    }
    println!("XR_DOCK_CATALOG apps={} pinned={}", apps.len(), pins.len());
    let mut model = Model::new(apps, pins);
    let stop = Arc::new(AtomicBool::new(false));
    let signal = stop.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    let started = Instant::now();
    let running = || {
        !stop.load(Ordering::Relaxed)
            && (options.seconds == 0 || started.elapsed() < Duration::from_secs(options.seconds))
    };
    let result = connected(&options.socket, &mut model, &running);
    model.disconnected();
    model.cancel();
    // A connection failure is distinct from an announced Shell shutdown.
    result?;
    println!("XR_DOCK_EXIT=0");
    Ok(())
}

fn connected(socket: &Path, model: &mut Model, running: &dyn Fn() -> bool) -> Result<()> {
    let mut client = Client::connect(
        socket,
        "dock",
        &[
            "core.v1",
            "scene.v1",
            "scene.spatial-policy.v1",
            "snapshot.v1",
            "input.v1",
            "window.v1",
            "workspace.v1",
            "system.v1",
        ],
    )?;
    println!(
        "XR_DOCK_CONNECTED component={}",
        client.welcome["component_id"]
    );
    let mut revision: u64 = client.welcome["scene_revision"]
        .as_str()
        .unwrap_or("0")
        .parse()?;
    let mut pending: Option<(u64, BTreeSet<String>)> = None;
    let mut accepted_nodes = BTreeSet::new();
    let mut dirty = true;
    let mut resync_requested = true;
    client.send("GetSnapshot", json!({}), &[])?;
    while running() {
        dirty |= model.tick_clock();
        if let Some(packet) = client.recv(100)? {
            if !packet.fds.is_empty() {
                bail!("unexpected FD on Dock control channel");
            }
            let kind = packet.message["type"]
                .as_str()
                .context("missing message type")?;
            let payload = &packet.message["payload"];
            match kind {
                "SessionEnding" => {
                    println!("XR_DOCK_SESSION_ENDING={payload}");
                    break;
                }
                "ComponentCommand" => {
                    dirty |= model.component_command(payload);
                }
                "Snapshot" | "StateChanged" => {
                    if model.snapshot(payload, kind == "StateChanged")? {
                        dirty = true;
                        resync_requested = false;
                        println!(
                            "XR_DOCK_SNAPSHOT revision={} workspace={} windows={}",
                            model.state_revision.unwrap_or(0),
                            model.active_workspace,
                            model.windows.len()
                        );
                    }
                    if !model.awaiting_snapshot {
                        resync_requested = false;
                    }
                    if model.awaiting_snapshot && !resync_requested {
                        client.send("GetSnapshot", json!({}), &[])?;
                        resync_requested = true;
                    }
                    if let Some(intent) = model.ready_intent() {
                        send_intent(&mut client, intent)?;
                    }
                }
                "SceneApplied" => {
                    let applied: u64 = payload["revision"]
                        .as_str()
                        .or_else(|| payload["scene_revision"].as_str())
                        .context("missing accepted scene revision")?
                        .parse()?;
                    if let Some((expected, nodes)) = pending.take() {
                        if applied != expected {
                            bail!(
                                "unexpected scene acknowledgement {applied}, expected {expected}"
                            );
                        }
                        revision = applied;
                        accepted_nodes = nodes;
                        println!(
                            "XR_DOCK_SCENE revision={revision} nodes={}",
                            accepted_nodes.len()
                        );
                    }
                }
                "SceneRejected" => {
                    let current = payload["current_revision"]
                        .as_str()
                        .or_else(|| payload["revision"].as_str())
                        .or_else(|| payload["scene_revision"].as_str())
                        .context("scene rejected without revision")?;
                    revision = current.parse()?;
                    pending = None;
                    dirty = true;
                    eprintln!("XR_DOCK_SCENE_REJECTED={payload}");
                }
                "InputEvent" | "Cancel" => {
                    if kind == "Cancel"
                        || matches!(payload["phase"].as_str(), Some("press" | "release"))
                    {
                        println!("XR_DOCK_INPUT={payload}");
                    }
                    let (changed, intent) = model.input(payload, kind == "Cancel");
                    dirty |= changed;
                    if let Some(intent) = intent {
                        send_intent(&mut client, intent)?;
                    }
                }
                "Error" => {
                    model.status = payload["message"]
                        .as_str()
                        .or_else(|| payload["description"].as_str())
                        .unwrap_or("操作失败")
                        .into();
                    dirty = true;
                    eprintln!("XR_DOCK_SERVER_ERROR={payload}");
                }
                "IntentResult" | "WindowIntentResult" | "WorkspaceApplied" => {
                    println!("XR_DOCK_RESULT={payload}")
                }
                _ => {}
            }
        }
        if dirty && pending.is_none() && !model.awaiting_snapshot {
            let scene = model.scene();
            let nodes: BTreeSet<_> = scene.widgets.iter().map(|w| w.id.clone()).collect();
            let mut operations = Vec::new();
            for id in accepted_nodes.difference(&nodes) {
                operations.push(json!({"op":"remove_node","node_id":id}));
            }
            operations.extend(scene.operations());
            let next = revision.checked_add(1).context("scene revision overflow")?;
            client.send("SceneTransaction", json!({"base_revision":revision.to_string(),"revision":next.to_string(),"operations":operations}), &[])?;
            pending = Some((next, nodes));
            dirty = false;
        }
    }
    model.cancel();
    // Closing the SDK connection lets Shell cancel captures and retire this scene.
    drop(client);
    println!("XR_DOCK_CONNECTION_CLOSED=1");
    Ok(())
}
fn send_intent(client: &mut Client, intent: Intent) -> Result<()> {
    let (kind, payload) = match intent {
        Intent::Launch(app_id) => ("WindowIntent", json!({"intent":"launch","app_id":app_id})),
        Intent::Focus(w) => (
            "WindowIntent",
            json!({"intent":"focus","surface_id":w.surface_id,"window_server_generation":w.window_server_generation}),
        ),
        Intent::Workspace(workspace) => ("WorkspaceIntent", json!({"workspace":workspace})),
    };
    let request = client.send(kind, payload.clone(), &[])?;
    println!("XR_DOCK_INTENT request={request} type={kind} payload={payload}");
    Ok(())
}
