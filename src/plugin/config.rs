// Adapted from zellij-vertical-tabs by Alex Lau at upstream commit
// 9b500a48427eed90654e5a226eae84908678ca92. See NOTICE and LICENSE.

use super::formatting::{FormatToken, StyledText, parse_styled_string, parse_tmux_format};
use std::collections::BTreeMap;

/// Styling configuration with static labels and borders compiled at load time.
#[derive(Clone)]
pub(super) struct StyleConfig {
    pub(super) format: Vec<FormatToken>,
    pub(super) format_active: Vec<FormatToken>,
    pub(super) overflow_above: String,
    pub(super) overflow_below: String,
    pub(super) indicator_active: String,
    pub(super) indicator_fullscreen: String,
    pub(super) indicator_sync: String,
    pub(super) padding_top: usize,
    pub(super) border: StyledText,
    pub(super) max_name_length: usize,
    pub(super) start_index: usize,
    pub(super) activity_format: String,
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            format: parse_tmux_format("{index}:{name}"),
            format_active: parse_tmux_format("{index}:{name} {indicators}"),
            overflow_above: "  ^ +{count}".to_string(),
            overflow_below: "  v +{count}".to_string(),
            indicator_active: "*".to_string(),
            indicator_fullscreen: "Z".to_string(),
            indicator_sync: "S".to_string(),
            max_name_length: 20,
            padding_top: 0,
            border: StyledText::new(),
            start_index: 1,
            activity_format: "#[fg=dim]{activity}".to_string(),
        }
    }
}

impl StyleConfig {
    pub(super) fn apply(&mut self, configuration: &BTreeMap<String, String>) {
        if let Some(v) = configuration.get("format") {
            self.format = parse_tmux_format(v);
        }
        if let Some(v) = configuration.get("format_active") {
            self.format_active = parse_tmux_format(v);
        }
        if let Some(v) = configuration.get("overflow_above") {
            self.overflow_above = v.clone();
        }
        if let Some(v) = configuration.get("overflow_below") {
            self.overflow_below = v.clone();
        }
        if let Some(v) = configuration.get("indicator_active") {
            self.indicator_active = v.clone();
        }
        if let Some(v) = configuration.get("indicator_fullscreen") {
            self.indicator_fullscreen = v.clone();
        }
        if let Some(v) = configuration.get("indicator_sync") {
            self.indicator_sync = v.clone();
        }
        if let Some(v) = configuration.get("max_name_length")
            && let Ok(n) = v.parse::<usize>()
        {
            self.max_name_length = n;
        }
        if let Some(v) = configuration.get("padding_top")
            && let Ok(n) = v.parse::<usize>()
        {
            self.padding_top = n;
        }
        if let Some(v) = configuration.get("border") {
            self.border = parse_styled_string(v);
        } else if let Some(v) = configuration.get("border_char") {
            self.border = parse_styled_string(v);
        }
        if let Some(v) = configuration.get("start_index")
            && let Ok(n) = v.parse::<usize>()
        {
            self.start_index = n;
        }
        if let Some(v) = configuration.get("activity_format") {
            self.activity_format = v.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::formatting::build_empty_line;

    #[test]
    fn repeated_configuration_preserves_style_values_and_prefers_border_over_alias() {
        let mut style = StyleConfig::default();
        style.apply(&BTreeMap::from([
            ("border".into(), "|".into()),
            ("border_char".into(), "!".into()),
            ("padding_top".into(), "2".into()),
            ("start_index".into(), "0".into()),
        ]));
        style.apply(&BTreeMap::from([("padding_top".into(), "invalid".into())]));

        assert_eq!(build_empty_line(&style.border, 1), "|");
        assert_eq!(style.padding_top, 2);
        assert_eq!(style.start_index, 0);
    }
}
