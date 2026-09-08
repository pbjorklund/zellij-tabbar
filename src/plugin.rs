// Adapted from zellij-vertical-tabs by Alex Lau at upstream commit
// 9b500a48427eed90654e5a226eae84908678ca92. See NOTICE and LICENSE.

mod config;
mod formatting;
mod rendering;

use self::config::StyleConfig;
use self::rendering::RenderContext;
use crate::{own_tab_is_active, scroll_target, select_active_tab};
use std::collections::BTreeMap;
use zellij_tile::prelude::*;

pub trait Host: Default {
    fn plugin_id(&mut self) -> u32;
    fn subscribe(&mut self, events: &[EventType]);
    fn request_permissions(&mut self, permissions: &[PermissionType]);
    fn set_selectable(&mut self, selectable: bool);
    fn switch_tab(&mut self, index: u32);
    fn render(&mut self, frame: &str);
    fn log(&mut self, _message: &str) {}
}

#[derive(Default)]
pub struct Tabbar<H: Host> {
    host: H,
    tabs: Vec<TabInfo>,
    active_tab_idx: usize,
    active_tab_id: Option<usize>,
    mode_info: ModeInfo,
    pane_manifest: PaneManifest,
    style: StyleConfig,
    permissions_granted: bool,
    is_selectable: bool,
    pending_events: Vec<Event>,
    activity: BTreeMap<String, activity::Activity>,
    own_session: String,
    own_plugin_id: Option<u32>,
    own_tab_position: Option<usize>,
    row_targets: Vec<Option<usize>>,
    diagnostics: bool,
    diagnostic_counts: BTreeMap<&'static str, u64>,
    render_size: Option<(usize, usize)>,
}

impl<H: Host> ZellijPlugin for Tabbar<H> {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.style.apply(&configuration);
        self.diagnostics = configuration
            .get("diagnostics")
            .is_some_and(|v| v == "true");
        self.own_plugin_id = Some(self.host.plugin_id());
        self.diagnose("loaded");

        // Subscribe before requesting permissions so a cached permission result
        // cannot arrive before this plugin is listening for it.
        self.host.subscribe(&[
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
            EventType::Mouse,
            EventType::PermissionRequestResult,
            EventType::SessionUpdate,
        ]);

        self.host.request_permissions(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        let mut should_render = false;

        if let Event::PermissionRequestResult(status) = event {
            if status == PermissionStatus::Granted {
                self.permissions_granted = true;
                self.is_selectable = false;
                self.host.set_selectable(false);
                self.diagnose("permission_granted");

                for cached_event in std::mem::take(&mut self.pending_events) {
                    self.update(cached_event);
                }
                should_render = true;
            } else {
                self.diagnose("permission_denied");
            }
            return should_render;
        }

        if !self.permissions_granted {
            self.pending_events.push(event);
            self.diagnose("permission_wait");
            return false;
        }

        match event {
            Event::PermissionRequestResult(_) => {}
            Event::ModeUpdate(mode_info) => {
                if self.mode_info != mode_info {
                    should_render = true;
                }
                self.mode_info = mode_info;
            }
            Event::TabUpdate(mut tabs) => {
                let missing_active = !tabs.is_empty() && !tabs.iter().any(|tab| tab.active);
                let tab_states: Vec<_> = tabs.iter().map(|tab| (tab.tab_id, tab.active)).collect();
                let active_tab = select_active_tab(
                    &tab_states,
                    self.active_tab_id,
                    self.active_tab_idx.saturating_sub(1),
                );
                let active_tab_idx = active_tab.map_or(0, |(index, _)| index + 1);
                let active_tab_id = active_tab.map(|(_, tab_id)| tab_id);
                // Keep rendering, labels, and navigation on the same fallback selection.
                if let Some((index, _)) = active_tab {
                    tabs[index].active = true;
                }
                if self.active_tab_idx != active_tab_idx || self.tabs != tabs {
                    should_render = true;
                }
                self.active_tab_idx = active_tab_idx;
                self.active_tab_id = active_tab_id;
                self.tabs = tabs;
                if missing_active {
                    self.diagnose("active_marker_missing");
                }
                if self.tabs.is_empty() {
                    self.diagnose("empty_tabs");
                }
            }
            Event::PaneUpdate(pane_manifest) => {
                self.own_tab_position = self.find_own_tab_position(&pane_manifest);
                should_render = self.pane_manifest != pane_manifest;
                self.pane_manifest = pane_manifest;
                if self.own_tab_position.is_none() {
                    self.diagnose("own_pane_missing");
                }
            }
            Event::Mouse(me) => match me {
                Mouse::LeftClick(row, _col) => {
                    if let Some(idx) = self.get_tab_at_row(row as usize) {
                        self.host.switch_tab(idx as u32);
                    }
                }
                Mouse::ScrollUp(_) => {
                    if let Some(target) = scroll_target(self.active_tab_idx, self.tabs.len(), false)
                    {
                        self.host.switch_tab(target as u32);
                    }
                }
                Mouse::ScrollDown(_) => {
                    if let Some(target) = scroll_target(self.active_tab_idx, self.tabs.len(), true)
                    {
                        self.host.switch_tab(target as u32);
                    }
                }
                _ => {}
            },
            Event::SessionUpdate(sessions, _) => {
                if let Some(s) = sessions.iter().find(|s| s.is_current_session)
                    && self.own_session != s.name
                {
                    self.own_session = s.name.clone();
                    should_render = true;
                }
            }
            _ => {}
        }
        let allowed = should_render && self.own_tab_is_active();
        self.diagnose(if allowed {
            "refresh_requested"
        } else if should_render {
            "refresh_suppressed"
        } else {
            "update_unchanged"
        });
        allowed
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        match pipe_message.name.as_str() {
            "set_selectable" => {
                match pipe_message.payload.as_deref() {
                    Some("true") => {
                        self.is_selectable = true;
                        self.host.set_selectable(true);
                    }
                    Some("false") => {
                        self.is_selectable = false;
                        self.host.set_selectable(false);
                    }
                    _ => {}
                }
                false
            }
            "toggle_selectable" => {
                self.is_selectable = !self.is_selectable;
                self.host.set_selectable(self.is_selectable);
                false
            }
            "activity" => {
                if let Some(payload) = pipe_message.payload.as_deref()
                    && let Some((zsession, name, act)) = activity::parse_activity(payload)
                {
                    self.activity.insert(format!("{zsession}\u{1}{name}"), act);
                    let allowed = self.own_tab_is_active();
                    self.diagnose(if allowed {
                        "activity_refresh"
                    } else {
                        "activity_suppressed"
                    });
                    return allowed;
                }
                self.diagnose("invalid_activity");
                false
            }
            _ => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        self.row_targets.clear();
        self.render_size = Some((rows, cols));
        if !self.permissions_granted || self.tabs.is_empty() {
            self.diagnose("render_skipped");
            return;
        }

        self.diagnose("render_start");
        let frame = RenderContext {
            tabs: &self.tabs,
            active_tab_idx: self.active_tab_idx,
            mode_info: &self.mode_info,
            pane_manifest: &self.pane_manifest,
            style: &self.style,
            activity: &self.activity,
            own_session: &self.own_session,
        }
        .render(rows, cols);
        self.row_targets = frame.row_targets;
        if let Some(text) = frame.text {
            self.host.render(&text);
        }
        self.diagnose("render");
    }
}

impl<H: Host> Tabbar<H> {
    fn diagnose(&mut self, event: &'static str) {
        if !self.diagnostics {
            return;
        }
        let count = self.diagnostic_counts.entry(event).or_default();
        *count = count.saturating_add(1);
        if !count.is_power_of_two() {
            return;
        }
        self.host.log(&format!(
            "zellij-tabbar version={} plugin_id={:?} event={event} count={count} permissions={} pending={} tabs={} active_tab_id={:?} own_tab_position={:?} size={:?} active_tab_position={:?}",
            env!("CARGO_PKG_VERSION"), self.own_plugin_id, self.permissions_granted,
            self.pending_events.len(), self.tabs.len(), self.active_tab_id,
            self.own_tab_position, self.render_size,
            self.tabs.iter().find(|tab| Some(tab.tab_id) == self.active_tab_id).map(|tab| tab.position),
        ));
    }

    fn own_tab_is_active(&self) -> bool {
        let tab_states: Vec<_> = self
            .tabs
            .iter()
            .map(|tab| (tab.position, tab.active))
            .collect();
        own_tab_is_active(&tab_states, self.own_tab_position)
    }

    fn find_own_tab_position(&self, pane_manifest: &PaneManifest) -> Option<usize> {
        pane_manifest
            .panes
            .iter()
            .find_map(|(tab_position, panes)| {
                panes
                    .iter()
                    .any(|pane| pane.is_plugin && Some(pane.id) == self.own_plugin_id)
                    .then_some(*tab_position)
            })
    }

    fn get_tab_at_row(&self, row: usize) -> Option<usize> {
        self.row_targets.get(row).copied().flatten()
    }
}

#[cfg(test)]
mod event_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestHost;

    impl Host for TestHost {
        fn plugin_id(&mut self) -> u32 {
            4
        }
        fn subscribe(&mut self, _: &[EventType]) {}
        fn request_permissions(&mut self, _: &[PermissionType]) {}
        fn set_selectable(&mut self, _: bool) {}
        fn switch_tab(&mut self, _: u32) {}
        fn render(&mut self, _: &str) {}
    }

    type State = Tabbar<TestHost>;

    #[test]
    fn zero_row_render_clears_targets_without_emitting_a_frame() {
        #[derive(Default)]
        struct NoFrameHost;
        impl Host for NoFrameHost {
            fn plugin_id(&mut self) -> u32 {
                4
            }
            fn subscribe(&mut self, _: &[EventType]) {}
            fn request_permissions(&mut self, _: &[PermissionType]) {}
            fn set_selectable(&mut self, _: bool) {}
            fn switch_tab(&mut self, _: u32) {}
            fn render(&mut self, _: &str) {
                panic!("zero rows must not emit a frame");
            }
        }
        let mut state = Tabbar::<NoFrameHost> {
            tabs: vec![TabInfo::default()],
            permissions_granted: true,
            row_targets: vec![Some(1)],
            ..Tabbar::default()
        };

        state.render(0, 10);

        assert!(state.row_targets.is_empty());
        assert_eq!(state.render_size, Some((0, 10)));
    }

    #[test]
    fn rendered_row_targets_drive_click_navigation() {
        let state = State {
            row_targets: vec![None, Some(2), Some(3), Some(4)],
            ..State::default()
        };

        assert_eq!(state.get_tab_at_row(0), None);
        assert_eq!(state.get_tab_at_row(1), Some(2));
        assert_eq!(state.get_tab_at_row(3), Some(4));
        assert_eq!(state.get_tab_at_row(4), None);
    }
}
