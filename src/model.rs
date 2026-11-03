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
        let expanded = self.menu_open;
        let height = if expanded { 620.0 } else { 180.0 };
        let bar_y = height - 180.0;
        let mut scene = Scene::new(
            [1200.0, height],
            [1.2, height / 1000.0],
            [0.0, 1.1, -1.5],
            "dock_lazy_follow",
        );
        let mut intents = BTreeMap::new();
        scene.push(Widget::card(
            "dock-background",
            [0.0, bar_y, 1200.0, 180.0],
            "dock.background",
        ));
        scene.push(Widget::button(
            "launcher",
            [16.0, bar_y + 16.0, 108.0, 76.0],
            "应用",
            "open-launcher",
            "dock.application",
        ));
        for (i, app) in self.pinned.iter().enumerate() {
            let id = app_node("pinned", &app.id);
            scene.push(
                Widget::button(
                    &id,
                    [136.0 + i as f32 * 108.0, bar_y + 16.0, 100.0, 76.0],
                    &short(&app.name, 5),
                    &id,
                    "dock.application",
                )
                .with_icon(&app.icon),
            );
            intents.insert(id, Intent::Launch(app.id.clone()));
        }
        scene.push(Widget::text(
            "workspace-label",
            [720.0, bar_y + 16.0, 462.0, 32.0],
            &format!("晴空 · 工作区 {}", self.active_workspace + 1),
            "text.primary",
        ));
        let active = self
            .windows
            .iter()
            .find(|w| w.focused)
            .map(|w| w.title.as_str())
            .unwrap_or("就绪");
        let status = if self.status.is_empty() {
            active
        } else {
            &self.status
        };
        scene.push(Widget::text(
            "dock-status",
            [720.0, bar_y + 54.0, 350.0, 32.0],
            &short(status, 28),
            "text.secondary",
        ));
        for space in 0..4 {
            let id = format!("workspace-{space}");
            let count = self.windows.iter().filter(|w| w.workspace == space).count();
            scene.push(Widget::button(
                &id,
                [16.0 + space as f32 * 68.0, bar_y + 112.0, 60.0, 50.0],
                &format!("{}·{}", space + 1, count),
                &id,
                if space == self.active_workspace {
                    "dock.active"
                } else {
                    "button.secondary"
                },
            ));
            intents.insert(id, Intent::Workspace(space));
        }
        let task_pages = self
            .windows
            .iter()
            .filter(|w| w.workspace == self.active_workspace)
            .count()
            .div_ceil(3)
            .max(1);
        self.task_page = self.task_page.min(task_pages - 1);
        scene.push(
            Widget::button(
                "tasks-prev",
                [1080.0, bar_y + 52.0, 44.0, 38.0],
                "‹",
                "tasks-prev",
                "button.secondary",
            )
            .enabled(self.task_page > 0),
        );
        scene.push(
            Widget::button(
                "tasks-next",
                [1136.0, bar_y + 52.0, 44.0, 38.0],
                "›",
                "tasks-next",
                "button.secondary",
            )
            .enabled(self.task_page + 1 < task_pages),
        );
        for (i, window) in self
            .windows
            .iter()
            .filter(|w| w.workspace == self.active_workspace)
            .skip(self.task_page * 3)
            .take(3)
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
            let x = 320.0 + i as f32 * 286.0;
            let title = if window.title.is_empty() {
                &window.app_id
            } else {
                &window.title
            };
            scene.push(Widget::button(
                &id,
                [x, bar_y + 112.0, 226.0, 50.0],
                &short(title, 14),
                &id,
                if window.focused {
                    "dock.active"
                } else {
                    "button.secondary"
                },
            ));
            scene.push(Widget::button(
                &close,
                [x + 232.0, bar_y + 112.0, 44.0, 50.0],
                "×",
                &close,
                "button.danger",
            ));
            intents.insert(id, Intent::Focus(window.clone()));
            intents.insert(close, Intent::Close(window.clone()));
        }
        if expanded {
            scene.push(Widget::card(
                "launcher-panel",
                [0.0, 0.0, 1200.0, 422.0],
                "panel.background",
            ));
            scene.push(Widget::text(
                "launcher-title",
                [24.0, 18.0, 600.0, 36.0],
                "全部应用",
                "text.primary",
            ));
            let pages = self.apps.len().div_ceil(12).max(1);
            self.page = self.page.min(pages - 1);
            scene.push(
                Widget::button(
                    "page-prev",
                    [860.0, 18.0, 84.0, 44.0],
                    "上页",
                    "page-prev",
                    "button.secondary",
                )
                .enabled(self.page > 0),
            );
            scene.push(Widget::text(
                "page-label",
                [956.0, 18.0, 100.0, 44.0],
                &format!("{}/{}", self.page + 1, pages),
                "text.secondary",
            ));
            scene.push(
                Widget::button(
                    "page-next",
                    [1072.0, 18.0, 104.0, 44.0],
                    "下页",
                    "page-next",
                    "button.secondary",
                )
                .enabled(self.page + 1 < pages),
            );
            if self.apps.is_empty() {
                scene.push(Widget::text(
                    "apps-empty",
                    [24.0, 100.0, 1120.0, 80.0],
                    "未找到可显示的应用",
                    "text.secondary",
                ));
            }
            for (i, app) in self.apps.iter().skip(self.page * 12).take(12).enumerate() {
                let id = app_node("application", &app.id);
                scene.push(
                    Widget::button(
                        &id,
                        [
                            24.0 + (i % 4) as f32 * 292.0,
                            86.0 + (i / 4) as f32 * 104.0,
                            276.0,
                            88.0,
                        ],
                        &short(&app.name, 17),
                        &id,
                        "dock.application",
                    )
                    .with_icon(&app.icon),
                );
                intents.insert(id, Intent::Launch(app.id.clone()));
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
