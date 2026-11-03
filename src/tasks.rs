// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{catalog::App, model::Window};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct TaskGroup {
    pub key: String,
    pub name: String,
    pub icon: String,
    pub windows: Vec<Window>,
}
impl TaskGroup {
    pub fn focused(&self) -> bool {
        self.windows.iter().any(|w| w.focused)
    }
}
fn normalized(id: &str) -> String {
    id.trim()
        .strip_suffix(".desktop")
        .unwrap_or(id.trim())
        .to_lowercase()
}
fn catalog_app<'a>(apps: &'a [App], app_id: &str) -> Option<&'a App> {
    let id = normalized(app_id);
    if id.is_empty() {
        return None;
    }
    if let Some(app) = apps.iter().find(|app| normalized(&app.id) == id) {
        return Some(app);
    }
    let mut aliases = apps
        .iter()
        .filter(|app| !app.startup_wm_class.is_empty() && normalized(&app.startup_wm_class) == id);
    if let Some(app) = aliases.next() {
        return if aliases.next().is_none() {
            Some(app)
        } else {
            None
        };
    }
    // Some X11 classes omit the desktop ID's namespace. Accept only an
    // unambiguous suffix, never a fuzzy substring or fabricated launch ID.
    let suffix = id.rsplit('.').next()?;
    let mut candidates = apps
        .iter()
        .filter(|app| normalized(&app.id).rsplit('.').next() == Some(suffix));
    let first = candidates.next()?;
    if candidates.next().is_none() {
        Some(first)
    } else {
        None
    }
}
pub fn groups(apps: &[App], pinned: &[App], windows: &[Window], active: u32) -> Vec<TaskGroup> {
    let mut groups: BTreeMap<String, TaskGroup> = BTreeMap::new();
    for window in windows {
        let app = catalog_app(apps, &window.app_id);
        let key = app.map(|a| a.id.clone()).unwrap_or_else(|| {
            if window.app_id.trim().is_empty() {
                format!(
                    "window:{}:{}",
                    window.window_server_generation, window.surface_id
                )
            } else {
                format!("running:{}", normalized(&window.app_id))
            }
        });
        let pin = pinned.iter().find(|a| a.id == key);
        let name = app.map(|a| a.name.as_str()).unwrap_or_else(|| {
            if window.app_id.is_empty() {
                &window.title
            } else {
                &window.app_id
            }
        });
        let icon = pin
            .or(app)
            .map(|a| a.icon.as_str())
            .filter(|icon| !icon.is_empty())
            .unwrap_or("application-x-executable");
        groups
            .entry(key.clone())
            .or_insert_with(|| TaskGroup {
                key,
                name: name.into(),
                icon: icon.into(),
                windows: Vec::new(),
            })
            .windows
            .push(window.clone());
    }
    let mut result: Vec<_> = groups.into_values().collect();
    for group in &mut result {
        group.windows.sort_by_key(|w| {
            (
                w.workspace != active,
                !w.focused,
                w.workspace,
                w.window_server_generation.clone(),
                w.surface_id.clone(),
            )
        });
    }
    result
}
