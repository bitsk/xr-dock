// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{
    catalog::App,
    clock::Clock,
    tasks::{self, TaskGroup},
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use xr_spatial_kit::{Action, InteractionState, Kind, Phase, PointerEvent, Scene, Widget, taskbar};

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
    group_actions: BTreeMap<String, String>,
    selected_group: Option<String>,
    window_page: usize,
    pending_focus: Option<(String, String)>,
    clock: Clock,
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
            group_actions: BTreeMap::new(),
            selected_group: None,
            window_page: 0,
            pending_focus: None,
            clock: Clock::new(),
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
    pub fn tick_clock(&mut self) -> bool {
        self.clock.refresh()
    }
    fn task_groups(&self) -> Vec<TaskGroup> {
        tasks::groups(
            &self.apps,
            &self.pinned,
            &self.windows,
            self.active_workspace,
        )
    }
    /// A focus in another workspace is sent only after its authoritative
    /// snapshot confirms the workspace transition, never optimistically.
    pub fn ready_intent(&mut self) -> Option<Intent> {
        if self.awaiting_snapshot {
            return None;
        }
        let (generation, surface) = self.pending_focus.as_ref()?;
        let window = self
            .windows
            .iter()
            .find(|w| &w.window_server_generation == generation && &w.surface_id == surface)
            .cloned();
        let Some(window) = window else {
            self.pending_focus = None;
            return None;
        };
        if window.workspace != self.active_workspace {
            return None;
        }
        self.pending_focus = None;
        Some(Intent::Focus(window))
    }
    fn choose_window(&mut self, target: Window) -> Option<Intent> {
        let window = self
            .windows
            .iter()
            .find(|w| {
                w.window_server_generation == target.window_server_generation
                    && w.surface_id == target.surface_id
            })?
            .clone();
        self.selected_group = None;
        if window.workspace != self.active_workspace {
            self.pending_focus = Some((
                window.window_server_generation.clone(),
                window.surface_id.clone(),
            ));
            Some(Intent::Workspace(window.workspace))
        } else {
            self.pending_focus = None;
            Some(Intent::Focus(window))
        }
    }
    fn activate_group(&mut self, key: &str) -> Option<Intent> {
        self.pending_focus = None;
        let group = self.task_groups().into_iter().find(|g| g.key == key)?;
        self.menu_open = false;
        if group.windows.len() == 1 {
            return self.choose_window(group.windows[0].clone());
        }
        if self.selected_group.as_deref() == Some(key) {
            self.selected_group = None;
        } else {
            self.selected_group = Some(key.into());
            self.window_page = 0;
        }
        None
    }
    pub fn scene(&mut self) -> Scene {
        let mut scene = Scene::new(
            taskbar::CENTER_LOGICAL,
            taskbar::CENTER_SIZE,
            taskbar::CENTER_POSITION,
            taskbar::POLICY,
        )
        .with_rotation(taskbar::ROTATION);
        let mut intents = BTreeMap::new();
        let mut group_actions = BTreeMap::new();
        scene.push(Widget::card(
            "dock-background",
            [
                0.0,
                0.0,
                taskbar::CENTER_LOGICAL[0],
                taskbar::LOGICAL_HEIGHT,
            ],
            "dock.background",
        ));
        scene.push(
            Widget::spatial_group(
                "data-island",
                taskbar::SIDE_LOGICAL,
                taskbar::SIDE_SIZE,
                taskbar::LEFT_POSITION,
            )
            .with_spatial_policy(taskbar::POLICY)
            .with_rotation(taskbar::ROTATION),
        );
        scene.push(
            Widget::card(
                "data-island-background",
                [0.0, 0.0, taskbar::SIDE_LOGICAL[0], taskbar::LOGICAL_HEIGHT],
                "dock.background",
            )
            .parent("data-island"),
        );
        scene.push(
            Widget::text(
                "local-clock",
                [24.0, 9.0, 132.0, 54.0],
                &self.clock.time,
                "taskbar.clock",
            )
            .parent("data-island"),
        );
        scene.push(
            Widget::card(
                "clock-separator",
                [168.0, 19.0, 1.0, 34.0],
                "taskbar.separator",
            )
            .parent("data-island"),
        );
        scene.push(
            Widget::text(
                "local-date",
                [186.0, 17.0, 142.0, 38.0],
                &self.clock.date_weekday,
                "taskbar.caption",
            )
            .parent("data-island"),
        );
        // No weather provider is installed: reserve the reference's weather slot
        // with an explicitly unavailable reading rather than a sample condition.
        scene.push(
            Widget::icon(
                "local-weather",
                [338., 19., 32., 32.],
                "ukui-xr-weather-unavailable",
                "taskbar.weather",
            )
            .parent("data-island"),
        );
        scene.push(
            Widget::text(
                "local-weather-temperature",
                [380., 16., 42., 40.],
                "—°",
                "taskbar.weather",
            )
            .parent("data-island"),
        );
        scene.push(
            Widget::button(
                "launcher",
                [18.0, 8.0, 56.0, 56.0],
                "",
                "open-launcher",
                "dock.app-icon",
            )
            .with_icon("ukui-xr-launcher"),
        );
        scene.push(Widget::card(
            "launcher-separator",
            [88.0, 18.0, 1.0, 36.0],
            "taskbar.separator",
        ));
        let groups = self.task_groups();
        let pinned_ids: BTreeSet<_> = self.pinned.iter().map(|p| p.id.as_str()).collect();
        let unpinned: Vec<_> = groups
            .iter()
            .filter(|g| !pinned_ids.contains(g.key.as_str()))
            .collect();
        let running_slots = 8usize.saturating_sub(self.pinned.len()).max(1);
        let task_pages = unpinned.len().div_ceil(running_slots).max(1);
        self.task_page = self.task_page.min(task_pages - 1);
        let visible_running: Vec<_> = unpinned
            .iter()
            .skip(self.task_page * running_slots)
            .take(running_slots)
            .collect();
        let visible_count = self.pinned.len() + visible_running.len();
        let gap = if visible_count > 1 {
            ((480.0 - visible_count as f32 * 56.0) / (visible_count - 1) as f32).clamp(0.0, 20.0)
        } else {
            0.0
        };
        let span = visible_count as f32 * 56.0 + visible_count.saturating_sub(1) as f32 * gap;
        let app_start = 110.0 + (480.0 - span) / 2.0;
        let app_step = 56.0 + gap;
        for (i, app) in self.pinned.iter().enumerate() {
            let id = app_node("dock-app", &app.id);
            let group = groups.iter().find(|g| g.key == app.id);
            app_button(
                &mut scene,
                &id,
                app_start + i as f32 * app_step,
                app_icon(app),
                group.is_some_and(|g| g.focused()),
                group.is_some(),
            );
            if let Some(group) = group {
                group_actions.insert(id, group.key.clone());
            } else {
                intents.insert(id, Intent::Launch(app.id.clone()));
            }
        }
        for (i, group) in visible_running.into_iter().enumerate() {
            let id = app_node("running-app", &group.key);
            app_button(
                &mut scene,
                &id,
                app_start + (self.pinned.len() + i) as f32 * app_step,
                &group.icon,
                group.focused(),
                true,
            );
            group_actions.insert(id, group.key.clone());
        }
        if task_pages > 1 {
            scene.push(
                Widget::button(
                    "tasks-prev",
                    [84.0, 24.0, 18.0, 24.0],
                    "",
                    "tasks-prev",
                    "dock.app-icon",
                )
                .with_icon("go-previous")
                .enabled(self.task_page > 0),
            );
            scene.push(
                Widget::button(
                    "tasks-next",
                    [598.0, 24.0, 16.0, 24.0],
                    "",
                    "tasks-next",
                    "dock.app-icon",
                )
                .with_icon("go-next")
                .enabled(self.task_page + 1 < task_pages),
            );
        }
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
                        [24.0 + i as f32 * 152.0, 64.0, 140.0, 68.0],
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
                    [24.0, 154.0, 320.0, 28.0],
                    "应用与工作区",
                    "text.secondary",
                )
                .parent(group),
            );
            for space in 0..4 {
                let id = format!("workspace-{space}");
                scene.push(
                    Widget::button(
                        &id,
                        [400.0 + space as f32 * 88.0, 142.0, 56.0, 40.0],
                        &(space + 1).to_string(),
                        &id,
                        if space == self.active_workspace {
                            "button.primary"
                        } else {
                            "button.secondary"
                        },
                    )
                    .parent(group),
                );
                intents.insert(id, Intent::Workspace(space));
            }
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
        if let Some(key) = self.selected_group.clone() {
            if let Some(group) = groups.iter().find(|g| g.key == key) {
                let panel = "window-picker";
                let pages = group.windows.len().div_ceil(6).max(1);
                self.window_page = self.window_page.min(pages - 1);
                scene.push(Widget::spatial_group(
                    panel,
                    [520.0, 400.0],
                    [0.65, 0.5],
                    [0.0, 1.2, -1.68],
                ));
                scene.push(
                    Widget::card(
                        "window-picker-background",
                        [0.0, 0.0, 520.0, 400.0],
                        "panel.background",
                    )
                    .parent(panel),
                );
                scene.push(
                    Widget::text(
                        "window-picker-title",
                        [20.0, 16.0, 412.0, 32.0],
                        &format!(
                            "{} · {} 个窗口",
                            short(&group.name, 18),
                            group.windows.len()
                        ),
                        "text.primary",
                    )
                    .parent(panel),
                );
                scene.push(
                    Widget::button(
                        "window-picker-close",
                        [456.0, 10.0, 44.0, 44.0],
                        "",
                        "window-picker-close",
                        "button.icon",
                    )
                    .with_icon("window-close")
                    .parent(panel),
                );
                for (i, window) in group
                    .windows
                    .iter()
                    .skip(self.window_page * 6)
                    .take(6)
                    .enumerate()
                {
                    let id = format!(
                        "pick-{}-{}",
                        window.window_server_generation, window.surface_id
                    );
                    let title = if window.title.is_empty() {
                        &window.app_id
                    } else {
                        &window.title
                    };
                    scene.push(
                        Widget::button(
                            &id,
                            [20.0, 64.0 + i as f32 * 44.0, 480.0, 40.0],
                            &format!("{} · 工作区 {}", short(title, 14), window.workspace + 1),
                            &id,
                            if window.focused {
                                "button.primary"
                            } else {
                                "button.secondary"
                            },
                        )
                        .with_icon(&group.icon)
                        .parent(panel),
                    );
                    intents.insert(id, Intent::Focus(window.clone()));
                }
                scene.push(
                    Widget::button(
                        "windows-prev",
                        [20.0, 348.0, 44.0, 36.0],
                        "",
                        "windows-prev",
                        "button.icon",
                    )
                    .with_icon("go-previous")
                    .enabled(self.window_page > 0)
                    .parent(panel),
                );
                scene.push(
                    Widget::text(
                        "windows-page",
                        [200.0, 352.0, 120.0, 28.0],
                        &format!("{}/{}", self.window_page + 1, pages),
                        "text.secondary",
                    )
                    .parent(panel),
                );
                scene.push(
                    Widget::button(
                        "windows-next",
                        [456.0, 348.0, 44.0, 36.0],
                        "",
                        "windows-next",
                        "button.icon",
                    )
                    .with_icon("go-next")
                    .enabled(self.window_page + 1 < pages)
                    .parent(panel),
                );
            } else {
                self.selected_group = None;
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
        self.group_actions = group_actions;
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
        if visible {
            self.selected_group = None;
            self.pending_focus = None;
        }
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
                    self.selected_group = None;
                    self.pending_focus = None;
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
                "window-picker-close" => {
                    self.selected_group = None;
                    return (true, None);
                }
                "windows-prev" => {
                    self.window_page = self.window_page.saturating_sub(1);
                    return (true, None);
                }
                "windows-next" => {
                    self.window_page += 1;
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
                    if let Some(key) = self.group_actions.get(id).cloned() {
                        let intent = self.activate_group(&key);
                        return (true, intent);
                    }
                    let intent = match self.intents.get(id).cloned() {
                        Some(Intent::Focus(window)) => self.choose_window(window),
                        Some(Intent::Workspace(workspace)) => {
                            self.pending_focus = None;
                            self.selected_group = None;
                            Some(Intent::Workspace(workspace))
                        }
                        other => other,
                    };
                    if matches!(intent, Some(Intent::Launch(_))) {
                        self.pending_focus = None;
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
        self.group_actions.clear();
        self.selected_group = None;
        self.pending_focus = None;
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

fn app_button(scene: &mut Scene, id: &str, x: f32, icon: &str, focused: bool, running: bool) {
    scene.push(
        Widget::button(
            id,
            [x, 6.0, 56.0, 56.0],
            "",
            id,
            if focused {
                "dock.active-icon"
            } else {
                "dock.app-icon"
            },
        )
        .with_icon(icon),
    );
    if running {
        scene.push(Widget::card(
            &format!("{id}-running"),
            [x + 25., 64.0, 6., 6.],
            "taskbar.running",
        ));
    }
}
