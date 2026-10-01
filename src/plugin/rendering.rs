// Adapted from zellij-vertical-tabs by Alex Lau at upstream commit
// 9b500a48427eed90654e5a226eae84908678ca92. See NOTICE and LICENSE.

use super::RowAction;
use super::config::StyleConfig;
use super::formatting::{
    FormatToken, InlineStyle, StyledText, build_empty_line, build_line, parse_styled_string,
};
use super::status::{AgentStatus, marker};
use crate::{calculate_visible_range, truncate_string};
use std::collections::BTreeMap;
use std::path::PathBuf;
use unicode_width::UnicodeWidthStr;
use zellij_tile::prelude::{InputMode, ModeInfo, PaneManifest, TabInfo};

pub(super) struct Frame {
    pub(super) text: Option<String>,
    pub(super) row_actions: Vec<Option<RowAction>>,
}

/// Immutable inputs for rendering, independent of callbacks and host effects.
pub(super) struct RenderContext<'a> {
    pub(super) tabs: &'a [TabInfo],
    pub(super) active_tab_idx: usize,
    pub(super) mode_info: &'a ModeInfo,
    pub(super) pane_manifest: &'a PaneManifest,
    pub(super) pane_cwds: &'a BTreeMap<u32, Option<PathBuf>>,
    pub(super) style: &'a StyleConfig,
    pub(super) activity: &'a BTreeMap<String, activity::Activity>,
    pub(super) statuses: &'a BTreeMap<u32, AgentStatus>,
    pub(super) status_frame: usize,
    pub(super) own_session: &'a str,
}

impl RenderContext<'_> {
    fn get_focused_pane_title(&self, tab_position: usize) -> Option<&str> {
        if let Some(panes) = self.pane_manifest.panes.get(&tab_position) {
            for pane in panes {
                if pane.is_focused && !pane.is_plugin {
                    let title = &pane.title;
                    if title.starts_with("Pane #") || title.starts_with("Tab #") || title.is_empty()
                    {
                        return None;
                    }
                    return Some(title);
                }
            }
        }
        None
    }

    fn focused_pane_directory(&self, tab_position: usize) -> Option<String> {
        let pane = self
            .pane_manifest
            .panes
            .get(&tab_position)?
            .iter()
            .find(|pane| pane.is_focused && !pane.is_plugin)?;
        let cwd = self.pane_cwds.get(&pane.id)?.as_ref()?;
        cwd.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .or_else(|| (cwd.as_os_str() == "/").then(|| "/".to_owned()))
    }

    fn status_for_tab(&self, tab_position: usize) -> Option<&AgentStatus> {
        self.pane_manifest
            .panes
            .get(&tab_position)
            .and_then(|panes| {
                panes.iter().find_map(|pane| {
                    (!pane.is_plugin)
                        .then(|| self.statuses.get(&pane.id))
                        .flatten()
                })
            })
    }

    fn expand_overflow_format(&self, format: &str, count: usize) -> String {
        format.replace("{count}", &count.to_string())
    }

    fn activity_for_tab(&self, tab: &TabInfo) -> Option<&activity::Activity> {
        let pane = self
            .get_focused_pane_title(tab.position)
            .map(norm_session_name)
            .unwrap_or_else(|| norm_session_name(&tab.name));
        let key = format!("{}\u{1}{}", self.own_session, pane);
        self.activity.get(&key)
    }

    /// Expand a tmux-style format string with tab info, returning styled text
    fn expand_tmux_format(
        &self,
        tokens: &[FormatToken],
        tab: &TabInfo,
        index: usize,
    ) -> StyledText {
        let mut result = StyledText::new();
        let mut current_style = InlineStyle::default();

        // Get focused pane title for this tab
        let pane_title = self
            .get_focused_pane_title(tab.position)
            .or_else(|| {
                if !tab.name.starts_with("Tab #") {
                    Some(tab.name.as_str())
                } else {
                    None
                }
            })
            .unwrap_or("...");

        // Build indicators string
        let mut indicators = String::new();
        if tab.is_fullscreen_active {
            indicators.push_str(&self.style.indicator_fullscreen);
        }
        if tab.is_sync_panes_active {
            indicators.push_str(&self.style.indicator_sync);
        }
        if tab.active {
            indicators.push_str(&self.style.indicator_active);
        }

        let mut prefix = String::new();
        if let Some(status) = self.status_for_tab(tab.position) {
            prefix.extend(
                "CIRS"
                    .chars()
                    .filter(|letter| status.watchers.contains(*letter)),
            );
            prefix.push_str(marker(status.mode, self.status_frame));
            if !prefix.is_empty() {
                prefix.push(' ');
            }
        }

        for token in tokens {
            match token {
                FormatToken::Style(style) => {
                    current_style = style.clone();
                }
                FormatToken::Variable { name, width } => {
                    let name_width = name
                        .split_once(':')
                        .filter(|(name, _)| matches!(*name, "name" | "n"))
                        .and_then(|(name, width)| {
                            width.parse::<usize>().ok().map(|width| (name, width))
                        });
                    let name = name_width.map_or(name.as_str(), |(name, _)| name);
                    let budget = width
                        .or(name_width.map(|(_, width)| width))
                        .unwrap_or(self.style.max_name_length);
                    let value = match name {
                        "index" | "i" => index.to_string(),
                        "name" | "n" => {
                            if tab.active
                                && self.mode_info.mode == InputMode::RenameTab
                                && tab.name.is_empty()
                            {
                                "Enter name...".to_string()
                            } else if !tab.name.starts_with("Tab #") && !tab.name.is_empty() {
                                tab.name.clone()
                            } else {
                                self.focused_pane_directory(tab.position)
                                    .unwrap_or_else(|| pane_title.to_owned())
                            }
                        }
                        "title" | "t" | "pane_title" => pane_title.to_owned(),
                        "indicators" => indicators.clone(),
                        "fullscreen" => {
                            if tab.is_fullscreen_active {
                                self.style.indicator_fullscreen.clone()
                            } else {
                                String::new()
                            }
                        }
                        "sync" => {
                            if tab.is_sync_panes_active {
                                self.style.indicator_sync.clone()
                            } else {
                                String::new()
                            }
                        }
                        "active" => {
                            if tab.active {
                                self.style.indicator_active.clone()
                            } else {
                                String::new()
                            }
                        }
                        _ => format!("{{{name}}}"),
                    };

                    let text = if matches!(name, "name" | "n") && !prefix.is_empty() {
                        let prefix_width = prefix.width();
                        if budget <= prefix_width {
                            let mut styled_prefix = StyledText::new();
                            styled_prefix.push(prefix.clone(), InlineStyle::default());
                            build_line(&styled_prefix, &StyledText::new(), budget, false)
                        } else {
                            format!("{prefix}{}", truncate_string(&value, budget - prefix_width))
                        }
                    } else {
                        truncate_string(&value, budget)
                    };
                    result.push(text, current_style.clone());
                }
                FormatToken::Literal(text) => {
                    result.push(text.clone(), current_style.clone());
                }
            }
        }

        result
    }

    pub(super) fn render(&self, rows: usize, cols: usize) -> Frame {
        let tabs = self.tabs;
        let rows_kept_for_tabs = usize::from(!tabs.is_empty());
        let tab_rows = rows;

        let top_padding = self
            .style
            .padding_top
            .min(tab_rows.saturating_sub(rows_kept_for_tabs));
        let available_rows = tab_rows.saturating_sub(top_padding);
        let active_index = tabs
            .iter()
            .position(|tab| tab.active)
            .unwrap_or_else(|| self.active_tab_idx.saturating_sub(1));
        let mut visible = calculate_visible_range(tabs.len(), available_rows, active_index);
        if visible.start == visible.end && !tabs.is_empty() && available_rows > 0 {
            visible.start = active_index.min(tabs.len() - 1);
            visible.end = visible.start + 1;
            visible.above = 0;
            visible.below = 0;
        }

        let mut lines: Vec<String> = Vec::with_capacity(rows);
        let mut row_actions: Vec<Option<RowAction>> = Vec::with_capacity(rows);

        for _ in 0..top_padding.min(tab_rows) {
            lines.push(build_empty_line(&self.style.border, cols));
            row_actions.push(None);
        }

        if visible.above > 0 && lines.len() < tab_rows {
            let indicator_text =
                self.expand_overflow_format(&self.style.overflow_above, visible.above);
            let styled = parse_styled_string(&indicator_text);
            lines.push(build_line(&styled, &self.style.border, cols, false));
            row_actions.push(
                tabs.get(visible.start.saturating_sub(1))
                    .and_then(switch_action),
            );
        }

        for i in visible.start..visible.end {
            if lines.len() >= tab_rows {
                break;
            }
            if let Some(tab) = tabs.get(i) {
                let is_active = tab.active;
                let format = if is_active {
                    &self.style.format_active
                } else {
                    &self.style.format
                };
                let styled = self.expand_tmux_format(
                    format,
                    tab,
                    tab.position.saturating_add(self.style.start_index),
                );
                let action = switch_action(tab);
                lines.push(build_line(&styled, &self.style.border, cols, is_active));
                row_actions.push(action);

                if let Some(act) = self.activity_for_tab(tab) {
                    let primary_rows_remaining =
                        visible.end.saturating_sub(i + 1) + usize::from(visible.below > 0);
                    let remaining_for_activity =
                        tab_rows.saturating_sub(lines.len() + primary_rows_remaining);
                    for activity_row in
                        activity::render_activity_limited(act, cols, remaining_for_activity)
                    {
                        let rendered = self
                            .style
                            .activity_format
                            .replace("{activity}", &activity_row);
                        let styled = parse_styled_string(&rendered);
                        lines.push(build_line(&styled, &self.style.border, cols, false));
                        row_actions.push(action);
                    }
                }
            }
        }

        if visible.below > 0 && lines.len() < tab_rows {
            let indicator_text =
                self.expand_overflow_format(&self.style.overflow_below, visible.below);
            let styled = parse_styled_string(&indicator_text);
            lines.push(build_line(&styled, &self.style.border, cols, false));
            row_actions.push(tabs.get(visible.end).and_then(switch_action));
        }

        while lines.len() < tab_rows {
            lines.push(build_empty_line(&self.style.border, cols));
            row_actions.push(None);
        }

        let text = if lines.is_empty() {
            None
        } else {
            let mut frame = lines.join("\x1b[m\n");
            frame.push_str("\x1b[m");
            Some(frame)
        };
        Frame { text, row_actions }
    }
}

fn switch_action(tab: &TabInfo) -> Option<RowAction> {
    Some(RowAction::SwitchTab {
        tab_id: u64::try_from(tab.tab_id).ok()?,
        position: tab.position,
    })
}

fn norm_session_name(s: &str) -> &str {
    let t = s.trim_start();
    if let Some(first) = t.chars().next()
        && !first.is_alphanumeric()
    {
        return t[first.len_utf8()..].trim_start();
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::RowAction;

    fn tab(id: usize, position: usize, active: bool) -> TabInfo {
        TabInfo {
            tab_id: id,
            position,
            active,
            name: format!("tab-{id}"),
            ..TabInfo::default()
        }
    }

    fn render(tabs: &[TabInfo], rows: usize) -> Frame {
        RenderContext {
            tabs,
            active_tab_idx: 1,
            mode_info: &ModeInfo::default(),
            pane_manifest: &PaneManifest::default(),
            pane_cwds: &BTreeMap::new(),
            style: &StyleConfig::default(),
            activity: &BTreeMap::new(),
            statuses: &BTreeMap::new(),
            status_frame: 0,
            own_session: "",
        }
        .render(rows, 24)
    }

    fn plain_lines(frame: &Frame) -> Vec<String> {
        frame
            .text
            .as_deref()
            .unwrap_or_default()
            .replace("\x1b[m", "")
            .lines()
            .map(|line| line.trim_end().to_string())
            .collect()
    }

    fn watcher_frame(
        mode: super::super::status::AgentMode,
        watchers: &str,
        name: &str,
        format: &str,
        budget: usize,
        cols: usize,
    ) -> Frame {
        use zellij_tile::prelude::PaneInfo;

        let tabs = [TabInfo {
            name: name.into(),
            ..tab(10, 0, true)
        }];
        let manifest = PaneManifest {
            panes: std::collections::HashMap::from([(
                0,
                vec![
                    PaneInfo {
                        id: 1,
                        is_plugin: true,
                        ..PaneInfo::default()
                    },
                    PaneInfo {
                        id: 2,
                        ..PaneInfo::default()
                    },
                    PaneInfo {
                        id: 3,
                        ..PaneInfo::default()
                    },
                    PaneInfo {
                        id: 4,
                        is_focused: true,
                        ..PaneInfo::default()
                    },
                ],
            )]),
        };
        let status = AgentStatus {
            runtime_id: "test".into(),
            seq: 1,
            mode,
            watchers: watchers.into(),
        };
        let statuses = BTreeMap::from([
            (
                1,
                AgentStatus {
                    watchers: "S".into(),
                    ..status.clone()
                },
            ),
            (3, status.clone()),
            (
                4,
                AgentStatus {
                    mode: super::super::status::AgentMode::Done,
                    watchers: "R".into(),
                    ..status
                },
            ),
        ]);
        let tokens = super::super::formatting::parse_tmux_format(format);
        let style = StyleConfig {
            format: tokens.clone(),
            format_active: tokens,
            max_name_length: budget,
            ..StyleConfig::default()
        };
        RenderContext {
            tabs: &tabs,
            active_tab_idx: 1,
            mode_info: &ModeInfo::default(),
            pane_manifest: &manifest,
            pane_cwds: &BTreeMap::new(),
            style: &style,
            activity: &BTreeMap::new(),
            statuses: &statuses,
            status_frame: 0,
            own_session: "",
        }
        .render(1, cols)
    }

    #[test]
    fn watcher_prefix_uses_first_status_pane_and_hides_p() {
        use super::super::status::AgentMode::{Base, Compacting, Done, Working};

        for (mode, watchers, expected) in [
            (Working, "CIPRS", "1:CIRS⠋ work"),
            (Base, "CIPRS", "1:CIRS work"),
            (Working, "P", "1:⠋ work"),
            (Base, "P", "1:work"),
            (Base, "", "1:work"),
            (Compacting, "CIPRS", "1:CIRS◐ work"),
            (Done, "CIPRS", "1:CIRS● work"),
        ] {
            let frame = watcher_frame(mode, watchers, "work", "{index}:{name}", 20, 24);
            assert_eq!(plain_lines(&frame), [expected]);
        }
    }

    #[test]
    fn watcher_prefix_is_reserved_before_name_truncation() {
        use super::super::status::AgentMode::Working;

        for (budget, expected) in [
            (0, ""),
            (1, "C"),
            (2, "CI"),
            (3, "CIR"),
            (4, "CIRS"),
            (5, "CIRS⠋"),
            (6, "CIRS⠋"),
            (7, "CIRS⠋ ."),
            (8, "CIRS⠋ .."),
            (10, "CIRS⠋ w..."),
        ] {
            let frame = watcher_frame(Working, "CIPRS", "work-long", "{name}", budget, 24);
            assert_eq!(plain_lines(&frame), [expected], "budget {budget}");
        }
        for (cols, expected) in [(0, ""), (1, "C"), (2, "CI"), (5, "CIRS⠋")] {
            let frame = watcher_frame(Working, "CIPRS", "work-long", "{name}", 20, cols);
            assert_eq!(
                plain_lines(&frame),
                if cols == 0 {
                    vec![]
                } else {
                    vec![expected.to_string()]
                }
            );
        }
    }

    #[test]
    fn watcher_prefix_respects_name_alias_width_and_unicode() {
        use super::super::status::AgentMode::Working;

        for (format, name, expected) in [
            ("{index} / {name:10}", "界界界界", "1 / CIRS⠋ ..."),
            ("{index} / {n:9}", "界e\u{301}", "1 / CIRS⠋ 界e\u{301}"),
            ("{name:8}", "界", "CIRS⠋ 界"),
            ("{=8:name}", "界", "CIRS⠋ 界"),
            ("{=1:i}:{name}", "work", "1:CIRS⠋ work"),
        ] {
            let frame = watcher_frame(Working, "CIPRS", name, format, 20, 30);
            assert_eq!(plain_lines(&frame), [expected]);
        }
    }

    #[test]
    fn watcher_prefix_preserves_active_fill_and_border() {
        use super::super::formatting::parse_tmux_format;
        use super::super::status::AgentMode::Working;
        use zellij_tile::prelude::PaneInfo;

        let tabs = [TabInfo {
            name: "work".into(),
            ..tab(10, 0, true)
        }];
        let manifest = PaneManifest {
            panes: std::collections::HashMap::from([(
                0,
                vec![PaneInfo {
                    id: 3,
                    ..PaneInfo::default()
                }],
            )]),
        };
        let statuses = BTreeMap::from([(
            3,
            AgentStatus {
                runtime_id: "test".into(),
                seq: 1,
                mode: Working,
                watchers: "CIPRS".into(),
            },
        )]);
        let style = StyleConfig {
            format_active: parse_tmux_format("#[bg=236,fill]{index} / {n}"),
            border: parse_styled_string("│"),
            ..StyleConfig::default()
        };
        let frame = RenderContext {
            tabs: &tabs,
            active_tab_idx: 1,
            mode_info: &ModeInfo::default(),
            pane_manifest: &manifest,
            pane_cwds: &BTreeMap::new(),
            style: &style,
            activity: &BTreeMap::new(),
            statuses: &statuses,
            status_frame: 0,
            own_session: "",
        }
        .render(1, 18);

        let text = frame.text.as_deref().unwrap();
        assert_eq!(
            text,
            "\x1b[7m\x1b[0m\x1b[7m\x1b[38;5;236m1\x1b[0m\x1b[7m\x1b[38;5;236m / \x1b[0m\x1b[7m\x1b[38;5;236mCIRS⠋ work   \x1b[0m│\x1b[m"
        );
        assert_eq!(
            frame.row_actions,
            [Some(RowAction::SwitchTab {
                tab_id: 10,
                position: 0
            })]
        );
    }

    #[test]
    fn rows_after_tabs_are_empty_and_have_no_click_target() {
        let tabs = [tab(10, 0, true), tab(20, 1, false)];
        let frame = render(&tabs, 6);
        assert_eq!(
            plain_lines(&frame),
            ["1:tab-10 *", "2:tab-20", "", "", "", ""]
        );
        assert!(frame.row_actions[2..].iter().all(Option::is_none));
    }

    #[test]
    fn top_padding_does_not_hide_the_only_tab_row() {
        let tabs = [tab(10, 0, true)];
        let style = StyleConfig {
            padding_top: 1,
            ..StyleConfig::default()
        };
        let frame = RenderContext {
            tabs: &tabs,
            active_tab_idx: 1,
            mode_info: &ModeInfo::default(),
            pane_manifest: &PaneManifest::default(),
            pane_cwds: &BTreeMap::new(),
            style: &style,
            activity: &BTreeMap::new(),
            statuses: &BTreeMap::new(),
            status_frame: 0,
            own_session: "",
        }
        .render(2, 24);

        assert_eq!(plain_lines(&frame), ["", "1:tab-10 *"]);
        assert_eq!(
            frame.row_actions[1],
            Some(RowAction::SwitchTab {
                tab_id: 10,
                position: 0,
            })
        );
    }
}
