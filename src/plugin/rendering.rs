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

        for token in tokens {
            match token {
                FormatToken::Style(style) => {
                    current_style = style.clone();
                }
                FormatToken::Variable { name, width } => {
                    let value = match name.as_str() {
                        "index" | "i" => index.to_string(),
                        "name" | "n" => {
                            let name = if tab.active
                                && self.mode_info.mode == InputMode::RenameTab
                                && tab.name.is_empty()
                            {
                                "Enter name...".to_string()
                            } else if !tab.name.starts_with("Tab #") && !tab.name.is_empty() {
                                tab.name.clone()
                            } else {
                                pane_title.to_owned()
                            };
                            let status = self
                                .status_for_tab(tab.position)
                                .map_or("", |status| marker(status.mode, self.status_frame));
                            if status.is_empty() {
                                name
                            } else {
                                format!("{status} {name}")
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

                    let budget = width.unwrap_or(self.style.max_name_length);
                    let suffix = if matches!(name.as_str(), "name" | "n") {
                        self.status_for_tab(tab.position)
                            .filter(|status| !status.watchers.is_empty())
                            .map(|status| format!(" {}", status.watchers))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let text = if suffix.len() >= budget {
                        let marker = self
                            .status_for_tab(tab.position)
                            .map_or("", |status| marker(status.mode, self.status_frame));
                        format!(
                            "{}{}",
                            marker,
                            truncate_string(&suffix, budget.saturating_sub(marker.chars().count()))
                        )
                    } else {
                        let name_budget = budget - suffix.len();
                        let name = if name_budget <= 2 {
                            self.status_for_tab(tab.position)
                                .map_or("", |status| marker(status.mode, self.status_frame))
                        } else {
                            &value
                        };
                        format!("{}{}", truncate_string(name, name_budget), suffix)
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
        let tabs: Vec<_> = self.tabs.iter().filter(|tab| !tab.is_parked).collect();
        let parked_tabs: Vec<_> = self.tabs.iter().filter(|tab| tab.is_parked).collect();
        let has_parked_header = rows >= 2;
        let rows_kept_for_tabs = usize::from(!tabs.is_empty());
        let parked_capacity = if has_parked_header {
            rows.saturating_sub(1 + rows_kept_for_tabs)
        } else {
            0
        };
        let parked_overflows = parked_tabs.len() > parked_capacity;
        let visible_parked_count = if parked_overflows {
            parked_capacity.saturating_sub(1)
        } else {
            parked_tabs.len()
        };
        let mut parked_lines = Vec::with_capacity(parked_capacity);
        let mut parked_actions = Vec::with_capacity(parked_capacity);
        for (index, tab) in parked_tabs.iter().take(visible_parked_count).enumerate() {
            let styled = self.expand_tmux_format(
                &self.style.format,
                tab,
                tab.position.saturating_add(self.style.start_index),
            );
            let action = resume_action(tab);
            parked_lines.push(build_line(&styled, &self.style.border, cols, false));
            parked_actions.push(action);

            let primary_rows_remaining =
                visible_parked_count.saturating_sub(index + 1) + usize::from(parked_overflows);
            let remaining_for_activity =
                parked_capacity.saturating_sub(parked_lines.len() + primary_rows_remaining);
            if let Some(activity) = self.activity_for_tab(tab) {
                for activity_row in
                    activity::render_activity_limited(activity, cols, remaining_for_activity)
                {
                    let rendered = self
                        .style
                        .activity_format
                        .replace("{activity}", &activity_row);
                    let styled = parse_styled_string(&rendered);
                    parked_lines.push(build_line(&styled, &self.style.border, cols, false));
                    parked_actions.push(action);
                }
            }
        }
        if parked_overflows && parked_capacity > 0 {
            let hidden = parked_tabs.len().saturating_sub(visible_parked_count);
            let indicator_text = self.expand_overflow_format(&self.style.overflow_below, hidden);
            let styled = parse_styled_string(&indicator_text);
            parked_lines.push(build_line(&styled, &self.style.border, cols, false));
            parked_actions.push(
                parked_tabs
                    .get(visible_parked_count)
                    .and_then(|tab| resume_action(tab)),
            );
        }
        let tab_rows = rows.saturating_sub(parked_lines.len() + usize::from(has_parked_header));

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
                    .and_then(|tab| switch_action(tab)),
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
            row_actions.push(tabs.get(visible.end).and_then(|tab| switch_action(tab)));
        }

        while lines.len() < tab_rows {
            lines.push(build_empty_line(&self.style.border, cols));
            row_actions.push(None);
        }

        if has_parked_header {
            lines.push(build_line(
                &self.style.parked_header,
                &self.style.border,
                cols,
                false,
            ));
            row_actions.push(Some(RowAction::ParkedHeader));
        }

        lines.extend(parked_lines);
        row_actions.extend(parked_actions);

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

fn resume_action(tab: &TabInfo) -> Option<RowAction> {
    Some(RowAction::ResumeTab {
        tab_id: u64::try_from(tab.tab_id).ok()?,
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

    fn tab(id: usize, position: usize, active: bool, is_parked: bool) -> TabInfo {
        TabInfo {
            tab_id: id,
            position,
            active,
            is_parked,
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

    #[test]
    fn parked_header_and_tabs_are_anchored_to_the_bottom() {
        let tabs = [tab(10, 0, true, false), tab(20, 1, false, true)];

        let frame = render(&tabs, 6);

        assert_eq!(
            plain_lines(&frame),
            ["1:tab-10 *", "", "", "", "Parked", "2:tab-20"]
        );
        assert_eq!(
            frame.row_actions,
            [
                Some(RowAction::SwitchTab {
                    tab_id: 10,
                    position: 0,
                }),
                None,
                None,
                None,
                Some(RowAction::ParkedHeader),
                Some(RowAction::ResumeTab { tab_id: 20 }),
            ]
        );
    }

    #[test]
    fn parked_rows_overflow_without_hiding_the_active_row_or_header() {
        let tabs = [
            tab(10, 0, true, false),
            tab(20, 1, false, true),
            tab(30, 2, false, true),
            tab(40, 3, false, true),
            tab(50, 4, false, true),
        ];

        let frame = render(&tabs, 4);

        assert_eq!(
            plain_lines(&frame),
            ["1:tab-10 *", "Parked", "2:tab-20", "  v +3"]
        );
        assert_eq!(
            frame.row_actions[3],
            Some(RowAction::ResumeTab { tab_id: 30 })
        );
    }

    #[test]
    fn two_rows_keep_the_active_row_and_parked_drop_header_available() {
        let tabs = [tab(10, 0, true, false), tab(20, 1, false, true)];

        let frame = render(&tabs, 2);

        assert_eq!(plain_lines(&frame), ["1:tab-10 *", "Parked"]);
        assert_eq!(frame.row_actions[1], Some(RowAction::ParkedHeader));
    }

    #[test]
    fn top_padding_does_not_hide_the_only_unparked_row() {
        let tabs = [tab(10, 0, true, false), tab(20, 1, false, true)];
        let style = StyleConfig {
            padding_top: 1,
            ..StyleConfig::default()
        };
        let frame = RenderContext {
            tabs: &tabs,
            active_tab_idx: 1,
            mode_info: &ModeInfo::default(),
            pane_manifest: &PaneManifest::default(),
            style: &style,
            activity: &BTreeMap::new(),
            statuses: &BTreeMap::new(),
            status_frame: 0,
            own_session: "",
        }
        .render(2, 24);

        assert_eq!(plain_lines(&frame), ["1:tab-10 *", "Parked"]);
        assert_eq!(
            frame.row_actions[0],
            Some(RowAction::SwitchTab {
                tab_id: 10,
                position: 0,
            })
        );
    }
}
