// Core behavior adapted from zellij-vertical-tabs by Alex Lau.
// Upstream commit: 9b500a48427eed90654e5a226eae84908678ca92.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRange {
    pub start: usize,
    pub end: usize,
    pub above: usize,
    pub below: usize,
}

pub fn calculate_visible_range(
    tab_count: usize,
    available_rows: usize,
    active_index: usize,
) -> VisibleRange {
    if tab_count == 0 {
        return VisibleRange {
            start: 0,
            end: 0,
            above: 0,
            below: 0,
        };
    }

    if tab_count <= available_rows {
        return VisibleRange {
            start: 0,
            end: tab_count,
            above: 0,
            below: 0,
        };
    }

    let max_visible = available_rows.saturating_sub(2);
    if max_visible == 0 {
        return VisibleRange {
            start: 0,
            end: 0,
            above: 0,
            below: tab_count,
        };
    }

    let active_index = active_index.min(tab_count - 1);
    let mut start = active_index;
    let mut end = active_index + 1;
    let mut room_left = max_visible.saturating_sub(1);
    let mut expand_below = false;

    while room_left > 0 {
        if !expand_below && start > 0 {
            start -= 1;
            room_left -= 1;
        } else if expand_below && end < tab_count {
            end += 1;
            room_left -= 1;
        } else if start > 0 {
            start -= 1;
            room_left -= 1;
        } else if end < tab_count {
            end += 1;
            room_left -= 1;
        } else {
            break;
        }
        expand_below = !expand_below;
    }

    VisibleRange {
        start,
        end,
        above: start,
        below: tab_count.saturating_sub(end),
    }
}

pub fn tab_target_at_row(
    tab_count: usize,
    rows: usize,
    padding_top: usize,
    active_index: usize,
    row: usize,
) -> Option<usize> {
    if tab_count == 0 || row >= rows || row < padding_top {
        return None;
    }

    let visible =
        calculate_visible_range(tab_count, rows.saturating_sub(padding_top), active_index);
    let mut cursor = padding_top;

    if visible.above > 0 {
        if row == cursor {
            return Some(visible.start);
        }
        cursor += 1;
    }

    let visible_tabs = visible.end.saturating_sub(visible.start);
    if row < cursor + visible_tabs {
        return Some(visible.start + (row - cursor) + 1);
    }
    cursor += visible_tabs;

    if visible.below > 0 && row == cursor {
        return Some((visible.end + 1).min(tab_count));
    }

    None
}

pub fn scroll_target(active_tab: usize, tab_count: usize, forward: bool) -> Option<usize> {
    if tab_count == 0 {
        return None;
    }

    let active_tab = active_tab.clamp(1, tab_count);
    Some(if forward {
        (active_tab + 1).min(tab_count)
    } else {
        active_tab.saturating_sub(1).max(1)
    })
}

pub fn select_active_tab(
    tabs: &[(usize, bool)],
    previous_active_id: Option<usize>,
    previous_active_index: usize,
) -> Option<(usize, usize)> {
    if let Some((index, (tab_id, _))) = tabs.iter().enumerate().find(|(_, (_, active))| *active) {
        return Some((index, *tab_id));
    }

    if let Some(previous_active_id) = previous_active_id
        && let Some(index) = tabs
            .iter()
            .position(|(tab_id, _)| *tab_id == previous_active_id)
    {
        return Some((index, previous_active_id));
    }

    let index = previous_active_index.min(tabs.len().checked_sub(1)?);
    Some((index, tabs[index].0))
}

pub fn truncate_string(value: &str, max_width: usize) -> String {
    if value.width() <= max_width {
        return value.to_string();
    }

    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let content_width = max_width - 3;
    let mut truncated = String::new();
    let mut width = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if width + character_width > content_width {
            break;
        }
        truncated.push(character);
        width += character_width;
    }
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_every_tab_when_rows_are_available() {
        assert_eq!(
            calculate_visible_range(3, 5, 1),
            VisibleRange {
                start: 0,
                end: 3,
                above: 0,
                below: 0,
            }
        );
    }

    #[test]
    fn keeps_active_tab_in_overflow_viewport() {
        assert_eq!(
            calculate_visible_range(8, 5, 4),
            VisibleRange {
                start: 3,
                end: 6,
                above: 3,
                below: 2,
            }
        );
    }

    #[test]
    fn maps_clicks_after_padding_and_overflow_indicator() {
        assert_eq!(tab_target_at_row(8, 6, 1, 4, 0), None);
        assert_eq!(tab_target_at_row(8, 6, 1, 4, 1), Some(3));
        assert_eq!(tab_target_at_row(8, 6, 1, 4, 2), Some(4));
    }

    #[test]
    fn maps_single_overflow_row_to_first_tab() {
        assert_eq!(tab_target_at_row(3, 1, 0, 1, 0), Some(1));
    }

    #[test]
    fn maps_below_overflow_click_to_next_hidden_tab() {
        assert_eq!(tab_target_at_row(8, 6, 1, 4, 5), Some(7));
    }

    #[test]
    fn scroll_navigation_is_clamped_to_existing_tabs() {
        assert_eq!(scroll_target(1, 3, false), Some(1));
        assert_eq!(scroll_target(2, 3, false), Some(1));
        assert_eq!(scroll_target(2, 3, true), Some(3));
        assert_eq!(scroll_target(3, 3, true), Some(3));
        assert_eq!(scroll_target(0, 0, true), None);
    }

    #[test]
    fn truncation_preserves_unicode_boundaries() {
        assert_eq!(truncate_string("räksmörgås", 7), "räks...");
        assert_eq!(truncate_string("界面", 3), "...");
        assert_eq!(truncate_string("界面界", 5), "界...");
    }

    #[test]
    fn preserves_active_tab_identity_when_an_update_has_no_active_marker() {
        let tabs = [(12, false), (13, false)];

        assert_eq!(select_active_tab(&tabs, Some(13), 3), Some((1, 13)));
        assert_eq!(select_active_tab(&tabs, Some(12), 1), Some((0, 12)));
    }

    #[test]
    fn active_marker_overrides_the_previous_tab_identity() {
        let tabs = [(12, false), (13, true)];

        assert_eq!(select_active_tab(&tabs, Some(12), 0), Some((1, 13)));
    }

    #[test]
    fn clamps_the_previous_index_when_the_active_tab_was_closed() {
        let tabs = [(12, false), (14, false)];

        assert_eq!(select_active_tab(&tabs, Some(13), 3), Some((1, 14)));
        assert_eq!(select_active_tab(&[], Some(13), 3), None);
    }
}
