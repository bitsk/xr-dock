// SPDX-License-Identifier: GPL-3.0-or-later
use crate::catalog::App;
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use xr_spatial_kit::{Action, InteractionState, Kind, Phase, PointerEvent, Scene, Widget};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Window {
    pub surface_id: String,
    pub window_server_generation: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub workspace: u32,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub role: String,
}
#[derive(Clone, Debug)]
pub enum Intent {
    Launch(String),
    Focus(Window),
    Close(Window),
    Workspace(u32),
    Component {
        role: String,
        panel: String,
        intent: String,
    },
}

pub struct Model {
    pub apps: Vec<App>,
    pub pinned: Vec<App>,
    pub windows: Vec<Window>,
    pub active_workspace: u32,
    pub state_revision: Option<u64>,
    pub awaiting_snapshot: bool,
    pub menu_open: bool,
    pub page: usize,
    pub task_page: usize,
    pub status: String,
    pub widgets: BTreeMap<String, Widget>,
    pub interactions: BTreeMap<String, InteractionState>,
    pub intents: BTreeMap<String, Intent>,
}
impl Model {
    pub fn new(apps: Vec<App>, pinned: Vec<App>) -> Self {
        Self {
            apps,
            pinned,
            windows: Vec::new(),
            active_workspace: 0,
            state_revision: None,
            awaiting_snapshot: true,
            menu_open: false,
            page: 0,
            task_page: 0,
            status: String::new(),
            widgets: BTreeMap::new(),
            interactions: BTreeMap::new(),
            intents: BTreeMap::new(),
        }
    }
    pub fn snapshot(&mut self, payload: &Value, incremental: bool) -> Result<bool> {
        let revision: u64 = payload["state_revision"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing state revision"))?
            .parse()?;
        if incremental {
            if self.awaiting_snapshot {
                return Ok(false);
            }
            if let Some(previous) = self.state_revision {
                if revision <= previous {
                    return Ok(false);
                }
                if revision != previous + 1 {
                    self.awaiting_snapshot = true;
                    return Ok(false);
                }
            }
        }
        let workspace = payload["active_workspace"].as_u64().unwrap_or(0);
        ensure!(workspace < 4, "unknown workspace");
        let windows: Vec<Window> =
            serde_json::from_value::<Vec<Window>>(payload["windows"].clone())?
                .into_iter()
                .filter(|w| w.role.is_empty() || w.role == "toplevel")
                .collect();
        let changed = self.state_revision.is_none()
            || windows != self.windows
            || workspace as u32 != self.active_workspace;
        if workspace as u32 != self.active_workspace {
            self.task_page = 0;
        }
        self.windows = windows;
        self.active_workspace = workspace as u32;
        self.state_revision = Some(revision);
        self.awaiting_snapshot = false;
        Ok(changed)
    }
    pub fn scene(&mut self) -> Scene {
        let mut scene = Scene::new(
            [1200.0, 64.0],
            [1.2, 0.064],
            [0.0, 0.86, -1.72],
            "dock_lazy_follow",
        )
        .with_rotation([12.0, 0.0, 0.0]);
        let mut intents = BTreeMap::new();
        scene.push(Widget::card(
            "dock-background",
            [0.0, 0.0, 1200.0, 64.0],
            "dock.background",
        ));
        for space in 0..4 {
            let id = format!("workspace-{space}");
            scene.push(Widget::button(
                &id,
                [14.0 + space as f32 * 39.0, 16.0, 32.0, 32.0],
                &(space + 1).to_string(),
                &id,
                if space == self.active_workspace {
                    "dock.active"
                } else {
                    "dock.workspace"
                },
            ));
            intents.insert(id, Intent::Workspace(space));
        }
        let tasks: Vec<_> = self
            .windows
            .iter()
            .filter(|w| w.workspace == self.active_workspace)
            .collect();
        let pages = tasks.len().div_ceil(4).max(1);
        self.task_page = self.task_page.min(pages - 1);
        if tasks.is_empty() {
            scene.push(
                Widget::button(
                    "launcher",
                    [562.0, 8.0, 48.0, 48.0],
                    "",
                    "open-launcher",
                    "button.icon",
                )
                .with_icon("view-app-grid"),
            );
        }
        if pages > 1 {
            scene.push(
                Widget::button(
                    "tasks-prev",
                    [176.0, 16.0, 28.0, 32.0],
                    "",
                    "tasks-prev",
                    "button.icon",
                )
                .with_icon("go-previous")
                .enabled(self.task_page > 0),
            );
            scene.push(
                Widget::button(
                    "tasks-next",
                    [1098.0, 16.0, 28.0, 32.0],
                    "",
                    "tasks-next",
                    "button.icon",
                )
                .with_icon("go-next")
                .enabled(self.task_page + 1 < pages),
            );
        }
        for (i, window) in tasks
            .into_iter()
            .skip(self.task_page * 4)
            .take(4)
            .enumerate()
        {
            let id = format!(
                "task-{}-{}",
                window.window_server_generation, window.surface_id
            );
            let close = format!(
                "close-{}-{}",
                window.window_server_generation, window.surface_id
            );
            let x = 210.0 + i as f32 * 220.0;
            let title = if window.title.is_empty() {
                &window.app_id
            } else {
                &window.title
            };
            let icon = self
                .apps
                .iter()
                .find(|a| {
                    a.id == window.app_id
                        || a.id.strip_suffix(".desktop") == Some(window.app_id.as_str())
                })
                .map(|a| a.icon.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("application-x-executable");
            scene.push(
                Widget::button(
                    &id,
                    [x, 8.0, 174.0, 48.0],
                    &short(title, 10),
                    &id,
                    if window.focused {
                        "dock.active"
                    } else {
                        "dock.task"
                    },
                )
                .with_icon(icon),
            );
            scene.push(
                Widget::button(
                    &close,
                    [x + 178.0, 20.0, 24.0, 24.0],
                    "",
                    &close,
                    "button.icon",
                )
                .with_icon("window-close"),
            );
            intents.insert(id, Intent::Focus(window.clone()));
            intents.insert(close, Intent::Close(window.clone()));
        }
        scene.push(
            Widget::button(
                "system",
                [1136.0, 8.0, 48.0, 48.0],
                "",
                "system",
                "dock.system",
            )
            .with_icon("preferences-system"),
        );
        intents.insert(
            "system".into(),
            Intent::Component {
                role: "system_ui".into(),
                panel: "control".into(),
                intent: "toggle".into(),
            },
        );
        if self.menu_open {
            let group = "launcher-space";
            scene.push(Widget::spatial_group(
                group,
                [960.0, 584.0],
                [0.96, 0.584],
                [0.0, 1.25, -1.68],
            ));
            scene.push(
                Widget::card(
                    "launcher-panel",
                    [0.0, 0.0, 960.0, 584.0],
                    "panel.background",
                )
                .parent(group),
            );
            scene.push(
                Widget::text(
                    "launcher-title",
                    [24.0, 18.0, 380.0, 30.0],
                    "应用库",
                    "text.primary",
                )
                .parent(group),
            );
            scene.push(
                Widget::button(
                    "launcher-close",
                    [892.0, 12.0, 44.0, 44.0],
                    "",
                    "launcher-close",
                    "button.icon",
                )
                .with_icon("window-close")
                .parent(group),
            );
            for (i, app) in self.pinned.iter().enumerate() {
                let id = app_node("pinned", &app.id);
                scene.push(
                    Widget::button(
                        &id,
                        [24.0 + i as f32 * 182.0, 64.0, 170.0, 68.0],
                        &short(&app.name, 8),
                        &id,
                        "dock.application",
                    )
                    .with_icon(app_icon(app))
                    .parent(group),
                );
                intents.insert(id, Intent::Launch(app.id.clone()));
            }
            let pages = self.apps.len().div_ceil(12).max(1);
            self.page = self.page.min(pages - 1);
            scene.push(
                Widget::text(
                    "apps-label",
                    [24.0, 154.0, 480.0, 28.0],
                    "全部应用",
                    "text.secondary",
                )
                .parent(group),
            );
            scene.push(
                Widget::button(
                    "page-prev",
                    [756.0, 148.0, 40.0, 36.0],
                    "",
                    "page-prev",
                    "button.icon",
                )
                .with_icon("go-previous")
                .enabled(self.page > 0)
                .parent(group),
            );
            scene.push(
                Widget::text(
                    "page-label",
                    [804.0, 154.0, 72.0, 28.0],
                    &format!("{}/{}", self.page + 1, pages),
                    "text.secondary",
                )
                .parent(group),
            );
            scene.push(
                Widget::button(
                    "page-next",
                    [892.0, 148.0, 40.0, 36.0],
                    "",
                    "page-next",
                    "button.icon",
                )
                .with_icon("go-next")
                .enabled(self.page + 1 < pages)
                .parent(group),
            );
            if self.apps.is_empty() {
                scene.push(
                    Widget::text(
                        "apps-empty",
                        [24.0, 218.0, 912.0, 40.0],
                        "未找到可显示的应用",
                        "text.secondary",
                    )
                    .parent(group),
                );
            }
            for (i, app) in self.apps.iter().skip(self.page * 12).take(12).enumerate() {
                let id = app_node("application", &app.id);
                scene.push(
                    Widget::button(
                        &id,
                        [
                            24.0 + (i % 4) as f32 * 230.0,
                            202.0 + (i / 4) as f32 * 106.0,
                            216.0,
                            88.0,
                        ],
                        &short(&app.name, 12),
                        &id,
                        "dock.application",
                    )
                    .with_icon(app_icon(app))
                    .parent(group),
                );
                intents.insert(id, Intent::Launch(app.id.clone()));
            }
            if !self.status.is_empty() {
                scene.push(
                    Widget::text(
                        "launcher-status",
                        [24.0, 540.0, 912.0, 28.0],
                        &short(&self.status, 52),
                        "text.secondary",
                    )
                    .parent(group),
                );
            }
        }
        let ids: BTreeSet<_> = scene.widgets.iter().map(|w| w.id.clone()).collect();
        self.interactions.retain(|id, _| ids.contains(id));
        self.widgets = scene
            .widgets
            .iter()
            .map(|w| (w.id.clone(), w.clone()))
            .collect();
        self.intents = intents;
        scene
    }
    pub fn component_command(&mut self, payload: &Value) -> bool {
        if payload["role"].as_str() != Some("dock") || payload["panel"].as_str() != Some("launcher")
        {
            return false;
        }
        let visible = match payload["intent"].as_str() {
            Some("show") => true,
            Some("hide") => false,
            Some("toggle") => !self.menu_open,
            _ => return false,
        };
        let changed = visible != self.menu_open;
        self.menu_open = visible;
        changed
    }
    pub fn input(&mut self, payload: &Value, cancel: bool) -> (bool, Option<Intent>) {
        let Some(id) = payload["target_node_id"].as_str() else {
            if cancel {
                self.cancel();
            }
            return (false, None);
        };
        let Some(widget) = self.widgets.get_mut(id) else {
            return (false, None);
        };
        if !matches!(widget.kind, Kind::Button | Kind::Slider) {
            return (false, None);
        }
        let phase = if cancel {
            Phase::Cancel
        } else {
            match payload["phase"].as_str().unwrap_or("") {
                "hover" => Phase::Hover,
                "press" => Phase::Press,
                "drag" => Phase::Drag,
                "release" => Phase::Release,
                _ => return (false, None),
            }
        };
        let event = PointerEvent {
            phase,
            interaction_id: payload["capture_id"]
                .as_str()
                .or_else(|| payload["interaction_id"].as_str())
                .unwrap_or("pointer")
                .into(),
            local: [
                payload["local_x"].as_f64().unwrap_or(-1.0) as f32,
                payload["local_y"].as_f64().unwrap_or(-1.0) as f32,
            ],
        };
        let state = self.interactions.entry(id.into()).or_default();
        if let Some(Action::Activated(_)) = state.apply(widget, &event) {
            self.status.clear();
            match id {
                "launcher" => {
                    self.menu_open = !self.menu_open;
                    return (true, None);
                }
                "launcher-close" => {
                    self.menu_open = false;
                    return (true, None);
                }
                "page-prev" => {
                    self.page = self.page.saturating_sub(1);
                    return (true, None);
                }
                "page-next" => {
                    self.page += 1;
                    return (true, None);
                }
                "tasks-prev" => {
                    self.task_page = self.task_page.saturating_sub(1);
                    return (true, None);
                }
                "tasks-next" => {
                    self.task_page += 1;
                    return (true, None);
                }
                _ => {
                    let intent = self.intents.get(id).cloned();
                    if matches!(intent, Some(Intent::Launch(_))) {
                        self.menu_open = false;
                    }
                    return (true, intent);
                }
            }
        }
        (false, None)
    }
    pub fn cancel(&mut self) {
        for state in self.interactions.values_mut() {
            state.cancel();
        }
    }
    pub fn disconnected(&mut self) {
        self.cancel();
        self.state_revision = None;
        self.awaiting_snapshot = true;
        self.windows.clear();
        self.widgets.clear();
        self.intents.clear();
        self.menu_open = false;
        self.status.clear();
    }
}
fn short(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.into()
    } else {
        text.chars().take(limit - 1).chain(['…']).collect()
    }
}
fn app_node(prefix: &str, id: &str) -> String {
    let hash = id.bytes().fold(0xcbf29ce484222325u64, |hash, b| {
        (hash ^ b as u64).wrapping_mul(0x100000001b3)
    });
    format!("{prefix}-{hash:016x}")
}

fn app_icon(app: &App) -> &str {
    if app.icon.is_empty() {
        "application-x-executable"
    } else {
        &app.icon
    }
}
