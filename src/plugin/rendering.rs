// Adapted from zellij-vertical-tabs by Alex Lau at upstream commit
// 9b500a48427eed90654e5a226eae84908678ca92. See NOTICE and LICENSE.

use super::config::StyleConfig;
use super::formatting::{
    FormatToken, InlineStyle, StyledText, build_empty_line, build_line, parse_styled_string,
};
use crate::{calculate_visible_range, truncate_string};
use std::collections::BTreeMap;
use zellij_tile::prelude::{InputMode, ModeInfo, PaneManifest, TabInfo};

pub(super) struct Frame {
    pub(super) text: Option<String>,
    pub(super) row_targets: Vec<Option<usize>>,
}

/// Immutable inputs for rendering, independent of callbacks and host effects.
pub(super) struct RenderContext<'a> {
    pub(super) tabs: &'a [TabInfo],
    pub(super) active_tab_idx: usize,
    pub(super) mode_info: &'a ModeInfo,
    pub(super) pane_manifest: &'a PaneManifest,
    pub(super) style: &'a StyleConfig,
    pub(super) activity: &'a BTreeMap<String, activity::Activity>,
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

    fn expand_overflow_format(&self, format: &str, count: usize) -> String {
        format.replace("{count}", &count.to_string())
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
                            if tab.active
                                && self.mode_info.mode == InputMode::RenameTab
                                && tab.name.is_empty()
                            {
                                "Enter name...".to_string()
                            } else if !tab.name.starts_with("Tab #") && !tab.name.is_empty() {
                                tab.name.clone()
                            } else {
                                pane_title.to_owned()
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

                    let text = if let Some(w) = width {
                        truncate_string(&value, *w)
                    } else {
                        truncate_string(&value, self.style.max_name_length)
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
        let top_padding = self.style.padding_top;
        let available_rows = rows.saturating_sub(top_padding);

        let tab_count = self.tabs.len();
        let active_index = self.active_tab_idx.saturating_sub(1);

        let visible = calculate_visible_range(tab_count, available_rows, active_index);

        let mut lines: Vec<String> = Vec::with_capacity(rows);
        let mut row_targets: Vec<Option<usize>> = Vec::with_capacity(rows);

        // Add top padding lines.
        for _ in 0..top_padding.min(rows) {
            lines.push(build_empty_line(&self.style.border, cols));
            row_targets.push(None);
        }

        // Render the "above" overflow indicator.
        if visible.above > 0 && lines.len() < rows {
            let indicator_text =
                self.expand_overflow_format(&self.style.overflow_above, visible.above);
            let styled = parse_styled_string(&indicator_text);
            lines.push(build_line(&styled, &self.style.border, cols, false));
            row_targets.push(Some(visible.start));
        }

        // Render visible tabs.
        for i in visible.start..visible.end {
            if lines.len() >= rows {
                break;
            }
            if let Some(tab) = self.tabs.get(i) {
                let is_active = tab.active;
                let format = if is_active {
                    &self.style.format_active
                } else {
                    &self.style.format
                };

                let styled = self.expand_tmux_format(format, tab, i + self.style.start_index);
                lines.push(build_line(&styled, &self.style.border, cols, is_active));
                row_targets.push(Some(i + 1));

                if self.activity.is_empty() {
                    continue;
                }
                let pane = self
                    .get_focused_pane_title(tab.position)
                    .map(norm_session_name)
                    .unwrap_or_else(|| norm_session_name(&tab.name));
                let key = format!("{}\u{1}{}", self.own_session, pane);
                if let Some(act) = self.activity.get(&key) {
                    let primary_rows_remaining =
                        visible.end.saturating_sub(i + 1) + usize::from(visible.below > 0);
                    let remaining_for_activity =
                        rows.saturating_sub(lines.len() + primary_rows_remaining);
                    for activity_row in
                        activity::render_activity_limited(act, cols, remaining_for_activity)
                    {
                        let rendered = self
                            .style
                            .activity_format
                            .replace("{activity}", &activity_row);
                        let styled = parse_styled_string(&rendered);
                        lines.push(build_line(&styled, &self.style.border, cols, false));
                        row_targets.push(Some(i + 1));
                    }
                }
            }
        }

        // Render the "below" overflow indicator.
        if visible.below > 0 && lines.len() < rows {
            let indicator_text =
                self.expand_overflow_format(&self.style.overflow_below, visible.below);
            let styled = parse_styled_string(&indicator_text);
            lines.push(build_line(&styled, &self.style.border, cols, false));
            row_targets.push(Some((visible.end + 1).min(tab_count)));
        }

        // Fill remaining rows with empty lines (just border).
        while lines.len() < rows {
            lines.push(build_empty_line(&self.style.border, cols));
            row_targets.push(None);
        }

        let text = if lines.is_empty() {
            None
        } else {
            let mut frame = lines.join("\x1b[m\n");
            frame.push_str("\x1b[m");
            Some(frame)
        };
        Frame { text, row_targets }
    }
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
