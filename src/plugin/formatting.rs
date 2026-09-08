// Adapted from zellij-vertical-tabs by Alex Lau at upstream commit
// 9b500a48427eed90654e5a226eae84908678ca92. See NOTICE and LICENSE.

use std::borrow::Cow;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    if !hex.is_ascii() {
        return None;
    }
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

/// Inline style from #[...] directive
#[derive(Debug, Clone, Default)]
pub(super) struct InlineStyle {
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
pub(super) struct StyledText {
    segments: Vec<StyledSegment>,
}

impl StyledText {
    pub(super) fn new() -> Self {
        Self { segments: vec![] }
    }

    pub(super) fn push(&mut self, text: String, style: InlineStyle) {
        if !text.is_empty() {
            let text = if text.chars().any(char::is_control) {
                text.chars()
                    .map(|ch| if ch.is_control() { ' ' } else { ch })
                    .collect()
            } else {
                text
            };
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

/// Token from parsing a tmux-style format string
#[derive(Debug, Clone)]
pub(super) enum FormatToken {
    /// Style directive: #[fg=color,bg=color,bold,dim]
    Style(InlineStyle),
    /// Variable with optional width: {var} or {=12:var}
    Variable { name: String, width: Option<usize> },
    /// Plain text
    Literal(String),
}

/// Parse a tmux-style format string into tokens
/// Supports: #[fg=color,bg=color,bold,dim], {variable}, {=width:variable}, #{variable}
pub(super) fn parse_tmux_format(format: &str) -> Vec<FormatToken> {
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

    let mut depth = 0usize;
    for part in content.split(|ch| match ch {
        '(' => {
            depth += 1;
            false
        }
        ')' => {
            depth = depth.saturating_sub(1);
            false
        }
        ',' => depth == 0,
        _ => false,
    }) {
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
pub(super) fn parse_styled_string(s: &str) -> StyledText {
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

fn fit_to_width(text: &StyledText, cols: usize) -> Cow<'_, StyledText> {
    if text.display_width() > cols {
        Cow::Owned(text.truncate(cols))
    } else {
        Cow::Borrowed(text)
    }
}

/// Build a complete line with content, padding, and border
pub(super) fn build_line(
    content: &StyledText,
    border: &StyledText,
    cols: usize,
    is_selected: bool,
) -> String {
    let border = fit_to_width(border, cols);
    let border_width = border.display_width();

    let effective_cols = cols.saturating_sub(border_width);

    // Truncate content if it exceeds available width to prevent wrapping
    let content = fit_to_width(content, effective_cols);
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

        line.extend(std::iter::repeat_n(' ', padding_needed));

        line.push_str("\x1b[0m");
    } else {
        // Normal rendering - bg colors only apply to text, not padding
        line.push_str(&content.to_ansi());

        line.extend(std::iter::repeat_n(' ', padding_needed));
    }

    // Add border (not affected by selection)
    if border_width > 0 {
        line.push_str(&border.to_ansi());
    }

    line
}

/// Build a line with just the border (for empty rows)
pub(super) fn build_empty_line(border: &StyledText, cols: usize) -> String {
    let border = fit_to_width(border, cols);
    let border_width = border.display_width();

    if border_width == 0 {
        return " ".repeat(cols);
    }

    let effective_cols = cols.saturating_sub(border_width);
    let mut line = " ".repeat(effective_cols);
    line.push_str(&border.to_ansi());
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_hex_colors_fall_back_without_panicking() {
        for value in ["#界", "#aé", "#界界", "#éabcd", "#ggg", "#12", "#1234567"] {
            assert_eq!(parse_color_spec(value), ColorSpec::Default, "{value}");
        }
    }

    #[test]
    fn raw_text_controls_cannot_change_terminal_rows_or_style() {
        let text = parse_styled_string("a\nb\rc\td\x1b[31m");
        assert_eq!(text.to_ansi(), "a b c d [31m");
    }

    #[test]
    fn rgb_functions_keep_commas_inside_style_attributes() {
        let text = parse_styled_string("#[fg=rgb(10, 20, 30),bg=rgb(40,50,60),bold]x");
        assert_eq!(
            text.to_ansi(),
            "\x1b[0m\x1b[1m\x1b[38;2;10;20;30m\x1b[48;2;40;50;60mx\x1b[0m"
        );
    }

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
        let content = parse_styled_string("#[bg=236,fg=252,bold,fill]1:code*");

        let line = build_line(&content, &StyledText::new(), 16, true);

        assert!(line.starts_with("\x1b[7m"));
        assert!(line.contains("1:code*"));
        assert!(line.ends_with("\x1b[0m"));
    }

    #[test]
    fn fill_and_border_preserve_exact_ansi_output() {
        let border = parse_styled_string("#[fg=240]│");
        let content = parse_styled_string("#[bg=236,fg=252,bold,fill]x");

        assert_eq!(
            build_line(&content, &border, 4, true),
            "\x1b[7m\x1b[0m\x1b[7m\x1b[1m\x1b[38;5;236m\x1b[48;5;252mx  \x1b[0m\x1b[0m\x1b[38;5;240m│\x1b[0m"
        );
        assert_eq!(
            build_line(&content, &border, 4, false),
            "\x1b[0m\x1b[1m\x1b[38;5;252m\x1b[48;5;236mx\x1b[0m  \x1b[0m\x1b[38;5;240m│\x1b[0m"
        );
    }
}
