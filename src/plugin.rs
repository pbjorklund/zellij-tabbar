// Adapted from zellij-vertical-tabs by Alex Lau at upstream commit
// 9b500a48427eed90654e5a226eae84908678ca92. See NOTICE and LICENSE.

use crate::{
    calculate_visible_range, own_tab_is_active, scroll_target, select_active_tab, truncate_string,
};
use std::borrow::Cow;
use std::collections::BTreeMap;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
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

// ========== COLOR SYSTEM ==========

/// Color specification supporting default, 256-color, and RGB
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum ColorSpec {
    /// Use terminal default color
    #[default]
    Default,
    /// 256-color palette index (0-255)
    EightBit(u8),
    /// True color RGB
    Rgb(u8, u8, u8),
}

impl ColorSpec {
    /// Generate ANSI escape code for foreground color
    fn to_ansi_fg(self) -> String {
        match self {
            ColorSpec::Default => String::new(),
            ColorSpec::EightBit(n) => format!("\x1b[38;5;{n}m"),
            ColorSpec::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
        }
    }

    /// Generate ANSI escape code for background color
    fn to_ansi_bg(self) -> String {
        match self {
            ColorSpec::Default => String::new(),
            ColorSpec::EightBit(n) => format!("\x1b[48;5;{n}m"),
            ColorSpec::Rgb(r, g, b) => format!("\x1b[48;2;{r};{g};{b}m"),
        }
    }

    fn is_default(self) -> bool {
        matches!(self, ColorSpec::Default)
    }
}

/// Parse a color value from string
/// Supports:
/// - Named colors: "accent", "dim", "red", etc.
/// - 256-color: "238"
/// - Hex RGB: "#444444" or "#444"
/// - RGB function: "rgb(68,68,68)"
fn parse_color_spec(name: &str) -> ColorSpec {
    let name = name.trim();

    // Check for RGB hex: #RGB or #RRGGBB
    if let Some(hex) = name.strip_prefix('#')
        && let Some((r, g, b)) = parse_hex_color(hex)
    {
        return ColorSpec::Rgb(r, g, b);
    }

    // Check for rgb(r,g,b) syntax
    if let Some(inner) = name.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')'))
        && let Some((r, g, b)) = parse_rgb_func(inner)
    {
        return ColorSpec::Rgb(r, g, b);
    }

    // Check for numeric 256-color
    if let Ok(n) = name.parse::<u8>() {
        return ColorSpec::EightBit(n);
    }

    // Named colors mapped to 256-color approximations
    match name.to_lowercase().as_str() {
        // Default/reset
        "none" | "default" | "reset" => ColorSpec::Default,

        // Theme-like semantic colors (mapped to reasonable 256-color values)
        "accent" | "primary" => ColorSpec::EightBit(39), // Bright blue
        "secondary" => ColorSpec::EightBit(75),          // Light blue
        "tertiary" => ColorSpec::EightBit(141),          // Purple
        "muted" | "quaternary" => ColorSpec::EightBit(245), // Light gray
        "dim" | "dimmed" => ColorSpec::EightBit(240),    // Dark gray

        // Standard colors
        "black" => ColorSpec::EightBit(0),
        "red" | "error" | "warning" => ColorSpec::EightBit(196),
        "green" | "success" | "ok" => ColorSpec::EightBit(82),
        "yellow" => ColorSpec::EightBit(226),
        "blue" => ColorSpec::EightBit(33),
        "magenta" => ColorSpec::EightBit(201),
        "cyan" => ColorSpec::EightBit(51),
        "white" => ColorSpec::EightBit(15),
        "orange" => ColorSpec::EightBit(208),
        "gray" | "grey" => ColorSpec::EightBit(244),
        "pink" => ColorSpec::EightBit(213),
        "purple" => ColorSpec::EightBit(135),

        // Unknown - use default
        _ => ColorSpec::Default,
    }
}

/// Parse hex color: "444444" or "444" -> (r, g, b)
fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    match hex.len() {
        3 => {
            // #RGB -> expand to #RRGGBB
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some((r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

/// Parse "r,g,b" -> (r, g, b)
fn parse_rgb_func(inner: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].trim().parse::<u8>().ok()?;
    let g = parts[1].trim().parse::<u8>().ok()?;
    let b = parts[2].trim().parse::<u8>().ok()?;
    Some((r, g, b))
}

// ========== STYLE SYSTEM ==========

/// Inline style from #[...] directive
#[derive(Debug, Clone, Default)]
struct InlineStyle {
    fg: ColorSpec,
    bg: ColorSpec,
    bold: bool,
    dim: bool,
    fill: bool,
}

impl InlineStyle {
    /// Generate ANSI escape codes for this style (without reverse - that's handled at line level)
    fn to_ansi(&self) -> String {
        let mut result = String::new();

        // Attributes
        if self.bold {
            result.push_str("\x1b[1m");
        }
        if self.dim {
            result.push_str("\x1b[2m");
        }

        // Colors
        result.push_str(&self.fg.to_ansi_fg());
        result.push_str(&self.bg.to_ansi_bg());

        result
    }

    fn has_any_style(&self) -> bool {
        !self.fg.is_default() || !self.bg.is_default() || self.bold || self.dim || self.fill
    }
}

/// A segment of text with styling
#[derive(Debug, Clone)]
struct StyledSegment {
    text: String,
    style: InlineStyle,
}

impl StyledSegment {
    fn display_width(&self) -> usize {
        self.text.width()
    }
}

/// Collection of styled segments forming a complete styled string
#[derive(Debug, Clone, Default)]
struct StyledText {
    segments: Vec<StyledSegment>,
}

impl StyledText {
    fn new() -> Self {
        Self { segments: vec![] }
    }

    fn push(&mut self, text: String, style: InlineStyle) {
        if !text.is_empty() {
            self.segments.push(StyledSegment { text, style });
        }
    }

    fn display_width(&self) -> usize {
        self.segments.iter().map(|s| s.display_width()).sum()
    }

    /// Render to ANSI-coded string
    fn to_ansi(&self) -> String {
        let mut result = String::new();

        for segment in &self.segments {
            if segment.style.has_any_style() {
                result.push_str("\x1b[0m"); // Reset before applying new style
                result.push_str(&segment.style.to_ansi());
            }
            result.push_str(&segment.text);
        }

        // Reset at end
        if self.segments.iter().any(|s| s.style.has_any_style()) {
            result.push_str("\x1b[0m");
        }

        result
    }

    /// Truncate to fit within max_width display columns
    fn truncate(&self, max_width: usize) -> StyledText {
        if self.display_width() <= max_width {
            return self.clone();
        }

        let mut result = StyledText::new();
        let mut remaining = max_width;

        for segment in &self.segments {
            if remaining == 0 {
                break;
            }

            let seg_width = segment.display_width();
            if seg_width <= remaining {
                result.push(segment.text.clone(), segment.style.clone());
                remaining -= seg_width;
            } else {
                // Truncate this segment
                let mut truncated = String::new();
                let mut width = 0;
                for ch in segment.text.chars() {
                    let ch_width = ch.width().unwrap_or(0);
                    if width + ch_width > remaining {
                        break;
                    }
                    truncated.push(ch);
                    width += ch_width;
                }
                result.push(truncated, segment.style.clone());
                break;
            }
        }

        result
    }
}

// ========== FORMAT PARSING ==========

/// Token from parsing a tmux-style format string
#[derive(Debug, Clone)]
enum FormatToken {
    /// Style directive: #[fg=color,bg=color,bold,dim]
    Style(InlineStyle),
    /// Variable with optional width: {var} or {=12:var}
    Variable { name: String, width: Option<usize> },
    /// Plain text
    Literal(String),
}

/// Parse a tmux-style format string into tokens
/// Supports: #[fg=color,bg=color,bold,dim], {variable}, {=width:variable}, #{variable}
fn parse_tmux_format(format: &str) -> Vec<FormatToken> {
    let mut tokens = Vec::new();
    let mut chars = format.chars().peekable();
    let mut literal = String::new();

    while let Some(ch) = chars.next() {
        if ch == '#' {
            match chars.peek() {
                Some('[') => {
                    // Flush literal
                    if !literal.is_empty() {
                        tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
                    }
                    chars.next(); // consume '['
                    // Parse style directive until ']'
                    let mut style_str = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == ']' {
                            chars.next();
                            break;
                        }
                        style_str.push(chars.next().unwrap());
                    }
                    tokens.push(FormatToken::Style(parse_style_directive(&style_str)));
                }
                Some('{') => {
                    // Flush literal
                    if !literal.is_empty() {
                        tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
                    }
                    chars.next(); // consume '{'
                    let var_token = parse_variable(&mut chars);
                    tokens.push(var_token);
                }
                _ => {
                    literal.push(ch);
                }
            }
        } else if ch == '{' {
            // Flush literal
            if !literal.is_empty() {
                tokens.push(FormatToken::Literal(std::mem::take(&mut literal)));
            }
            let var_token = parse_variable(&mut chars);
            tokens.push(var_token);
        } else {
            literal.push(ch);
        }
    }

    if !literal.is_empty() {
        tokens.push(FormatToken::Literal(literal));
    }

    tokens
}

/// Parse style directive content: "fg=color,bg=color,bold,dim"
fn parse_style_directive(content: &str) -> InlineStyle {
    let mut style = InlineStyle::default();

    for part in content.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some(color_str) = part.strip_prefix("fg=") {
            style.fg = parse_color_spec(color_str);
        } else if let Some(color_str) = part.strip_prefix("bg=") {
            style.bg = parse_color_spec(color_str);
        } else if part == "bold" {
            style.bold = true;
        } else if part == "dim" {
            style.dim = true;
        } else if part == "fill" {
            style.fill = true;
        } else if part == "default" || part == "none" || part == "reset" {
            style = InlineStyle::default();
        }
    }

    style
}

/// Parse variable content after '{': "var}" or "=12:var}"
fn parse_variable(chars: &mut std::iter::Peekable<std::str::Chars>) -> FormatToken {
    let mut content = String::new();
    while let Some(&c) = chars.peek() {
        if c == '}' {
            chars.next();
            break;
        }
        content.push(chars.next().unwrap());
    }

    // Check for width specifier: =12:varname
    if let Some(rest) = content.strip_prefix('=')
        && let Some(colon_pos) = rest.find(':')
    {
        let width_str = &rest[..colon_pos];
        let var_name = &rest[colon_pos + 1..];
        if let Ok(width) = width_str.parse::<usize>() {
            return FormatToken::Variable {
                name: var_name.to_string(),
                width: Some(width),
            };
        }
    }

    FormatToken::Variable {
        name: content,
        width: None,
    }
}

/// Parse a styled string like "#[fg=240]│" into StyledText
fn parse_styled_string(s: &str) -> StyledText {
    let tokens = parse_tmux_format(s);
    let mut result = StyledText::new();
    let mut current_style = InlineStyle::default();

    for token in tokens {
        match token {
            FormatToken::Style(style) => {
                current_style = style;
            }
            FormatToken::Literal(text) => {
                result.push(text, current_style.clone());
            }
            FormatToken::Variable { name, .. } => {
                // Variables in border strings are not expanded, treat as literal
                result.push(format!("{{{name}}}"), current_style.clone());
            }
        }
    }

    result
}

// ========== CONFIGURATION ==========

/// Styling configuration for tab labels
#[derive(Clone)]
struct StyleConfig {
    format: Vec<FormatToken>,
    format_active: Vec<FormatToken>,
    overflow_above: String,
    overflow_below: String,
    indicator_active: String,
    indicator_fullscreen: String,
    indicator_sync: String,
    padding_top: usize,
    border: StyledText,
    max_name_length: usize,
    start_index: usize,
    activity_format: String,
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

// ========== PLUGIN STATE ==========

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
        // Parse style configuration
        if let Some(v) = configuration.get("format") {
            self.style.format = parse_tmux_format(v);
        }
        if let Some(v) = configuration.get("format_active") {
            self.style.format_active = parse_tmux_format(v);
        }
        if let Some(v) = configuration.get("overflow_above") {
            self.style.overflow_above = v.clone();
        }
        if let Some(v) = configuration.get("overflow_below") {
            self.style.overflow_below = v.clone();
        }
        if let Some(v) = configuration.get("indicator_active") {
            self.style.indicator_active = v.clone();
        }
        if let Some(v) = configuration.get("indicator_fullscreen") {
            self.style.indicator_fullscreen = v.clone();
        }
        if let Some(v) = configuration.get("indicator_sync") {
            self.style.indicator_sync = v.clone();
        }
        if let Some(v) = configuration.get("max_name_length")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.max_name_length = n;
        }
        if let Some(v) = configuration.get("padding_top")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.padding_top = n;
        }
        if let Some(v) = configuration.get("border") {
            self.style.border = parse_styled_string(v);
        } else if let Some(v) = configuration.get("border_char") {
            self.style.border = parse_styled_string(v);
        }
        if let Some(v) = configuration.get("start_index")
            && let Ok(n) = v.parse::<usize>()
        {
            self.style.start_index = n;
        }
        if let Some(v) = configuration.get("activity_format") {
            self.style.activity_format = v.clone();
        }

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
        self.render_vertical(rows, cols);
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

    fn get_focused_pane_title(&self, tab_position: usize) -> Option<String> {
        if let Some(panes) = self.pane_manifest.panes.get(&tab_position) {
            for pane in panes {
                if pane.is_focused && !pane.is_plugin {
                    let title = &pane.title;
                    if title.starts_with("Pane #") || title.starts_with("Tab #") || title.is_empty()
                    {
                        return None;
                    }
                    return Some(title.clone());
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
                    Some(tab.name.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "...".to_string());

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
                                pane_title.clone()
                            }
                        }
                        "title" | "t" | "pane_title" => pane_title.clone(),
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

    fn border_at_width(&self, cols: usize) -> Cow<'_, StyledText> {
        if self.style.border.display_width() > cols {
            Cow::Owned(self.style.border.truncate(cols))
        } else {
            Cow::Borrowed(&self.style.border)
        }
    }

    /// Build a complete line with content, padding, and border
    fn build_line(&self, content: &StyledText, cols: usize, is_selected: bool) -> String {
        let border = self.border_at_width(cols);
        let border_width = border.display_width();

        let effective_cols = cols.saturating_sub(border_width);

        // Truncate content if it exceeds available width to prevent wrapping
        let content = content.truncate(effective_cols);
        let content_width = content.display_width();
        let padding_needed = effective_cols.saturating_sub(content_width);

        let mut line = String::new();

        // Check if any segment has fill attribute - fills entire row with bg color
        let has_fill = is_selected && content.segments.iter().any(|s| s.style.fill);

        if has_fill {
            // Fill mode: use reverse video with swapped colors so bg fills the row
            // User writes #[bg=236,fill] -> we swap to fg=236 -> reverse makes displayed bg=236
            line.push_str("\x1b[7m");

            for segment in &content.segments {
                // Swap fg and bg for reverse video
                let mut swapped_style = segment.style.clone();
                std::mem::swap(&mut swapped_style.fg, &mut swapped_style.bg);
                swapped_style.fill = false; // Don't need fill flag in output

                if swapped_style.has_any_style() {
                    line.push_str("\x1b[0m\x1b[7m"); // Reset and re-apply reverse
                    line.push_str(&swapped_style.to_ansi());
                }
                line.push_str(&segment.text);
            }

            if padding_needed > 0 {
                line.push_str(&" ".repeat(padding_needed));
            }

            line.push_str("\x1b[0m");
        } else {
            // Normal rendering - bg colors only apply to text, not padding
            line.push_str(&content.to_ansi());

            if padding_needed > 0 {
                line.push_str(&" ".repeat(padding_needed));
            }
        }

        // Add border (not affected by selection)
        if border_width > 0 {
            line.push_str(&border.to_ansi());
        }

        line
    }

    /// Build a line with just the border (for empty rows)
    fn build_empty_line(&self, cols: usize) -> String {
        let border = self.border_at_width(cols);
        let border_width = border.display_width();

        if border_width == 0 {
            return " ".repeat(cols);
        }

        let effective_cols = cols.saturating_sub(border_width);
        let mut line = " ".repeat(effective_cols);
        line.push_str(&border.to_ansi());
        line
    }

    fn render_vertical(&mut self, rows: usize, cols: usize) {
        let top_padding = self.style.padding_top;
        let available_rows = rows.saturating_sub(top_padding);

        let tab_count = self.tabs.len();
        let active_index = self.active_tab_idx.saturating_sub(1);

        let visible = calculate_visible_range(tab_count, available_rows, active_index);

        let mut lines: Vec<String> = Vec::with_capacity(rows);
        let mut row_targets: Vec<Option<usize>> = Vec::with_capacity(rows);

        // Add top padding lines.
        for _ in 0..top_padding.min(rows) {
            lines.push(self.build_empty_line(cols));
            row_targets.push(None);
        }

        // Render the "above" overflow indicator.
        if visible.above > 0 && lines.len() < rows {
            let indicator_text =
                self.expand_overflow_format(&self.style.overflow_above, visible.above);
            let styled = parse_styled_string(&indicator_text);
            lines.push(self.build_line(&styled, cols, false));
            row_targets.push(Some(visible.start));
        }

        // Render visible tabs.
        for i in visible.start..visible.end {
            if lines.len() >= rows {
                break;
            }
            if let Some(tab) = self.tabs.get(i).cloned() {
                let is_active = tab.active;
                let format = if is_active {
                    &self.style.format_active
                } else {
                    &self.style.format
                };

                let styled = self.expand_tmux_format(format, &tab, i + self.style.start_index);
                lines.push(self.build_line(&styled, cols, is_active));
                row_targets.push(Some(i + 1));

                let pane = self
                    .get_focused_pane_title(tab.position)
                    .map(|t| norm_session_name(&t))
                    .unwrap_or_else(|| norm_session_name(&tab.name));
                let key = format!("{}\u{1}{}", self.own_session, pane);
                if let Some(act) = self.activity.get(&key) {
                    let primary_rows_remaining =
                        visible.end.saturating_sub(i + 1) + usize::from(visible.below > 0);
                    for activity_row in activity::render_activity(act, cols) {
                        if lines.len() + primary_rows_remaining >= rows {
                            break;
                        }
                        let rendered = self
                            .style
                            .activity_format
                            .replace("{activity}", &activity_row);
                        let styled = parse_styled_string(&rendered);
                        lines.push(self.build_line(&styled, cols, false));
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
            lines.push(self.build_line(&styled, cols, false));
            row_targets.push(Some((visible.end + 1).min(tab_count)));
        }

        // Fill remaining rows with empty lines (just border).
        while lines.len() < rows {
            lines.push(self.build_empty_line(cols));
            row_targets.push(None);
        }

        self.row_targets = row_targets;

        if !lines.is_empty() {
            let mut frame = lines.join("\x1b[m\n");
            frame.push_str("\x1b[m");
            self.host.render(&frame);
        }
    }

    fn get_tab_at_row(&self, row: usize) -> Option<usize> {
        self.row_targets.get(row).copied().flatten()
    }
}

fn norm_session_name(s: &str) -> String {
    let t = s.trim_start();
    let mut chars = t.chars();
    if let Some(first) = chars.clone().next()
        && !first.is_alphanumeric()
    {
        return chars
            .by_ref()
            .skip(1)
            .collect::<String>()
            .trim_start()
            .to_string();
    }
    t.to_string()
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
    fn parses_named_indexed_and_rgb_colors() {
        assert_eq!(parse_color_spec("accent"), ColorSpec::EightBit(39));
        assert_eq!(parse_color_spec("236"), ColorSpec::EightBit(236));
        assert_eq!(parse_color_spec("#abc"), ColorSpec::Rgb(170, 187, 204));
        assert_eq!(
            parse_color_spec("rgb(10, 20, 30)"),
            ColorSpec::Rgb(10, 20, 30)
        );
    }

    #[test]
    fn styled_text_truncates_on_unicode_column_boundaries() {
        let mut text = StyledText::new();
        text.push("a界b".to_string(), InlineStyle::default());

        let truncated = text.truncate(3);

        assert_eq!(truncated.display_width(), 3);
        assert_eq!(truncated.segments[0].text, "a界");
    }

    #[test]
    fn active_fill_style_covers_the_complete_row() {
        let state = State::default();
        let content = parse_styled_string("#[bg=236,fg=252,bold,fill]1:code*");

        let line = state.build_line(&content, 16, true);

        assert!(line.starts_with("\x1b[7m"));
        assert!(line.contains("1:code*"));
        assert!(line.ends_with("\x1b[0m"));
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
