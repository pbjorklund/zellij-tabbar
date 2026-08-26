# zellij-tabbar

`zellij-tabbar` shows Zellij tabs in a left sidebar. The active row stays visible, long names are truncated on Unicode display boundaries, and hidden tabs use overflow rows.

It supports mouse clicks, wheel navigation, pane-title formats, active-row fill, 8-bit colors, RGB colors, and configurable indicators. `examples/vertical-tabs-left.kdl` uses the same 32-column style as the Omarchy layout.

## Install a release

Download `zellij-tabbar.wasm` and `SHA256SUMS` from the matching release, then verify and install the plugin:

```sh
sha256sum --check SHA256SUMS
mkdir -p ~/.config/zellij/plugins
install -m 0644 zellij-tabbar.wasm ~/.config/zellij/plugins/zellij-tabbar.wasm
```

Copy one example layout into Zellij's layout directory:

```sh
mkdir -p ~/.config/zellij/layouts
cp examples/vertical-tabs-left.kdl ~/.config/zellij/layouts/
zellij --layout vertical-tabs-left
```

Use `examples/vertical-tabs-left.swap.kdl` with Zellij's swap-layout support. Use `examples/horizontal-tabs.kdl` as a narrow-terminal fallback with Zellij's built-in horizontal tab bar.

## Permissions

The plugin requests two Zellij permissions on first use:

- `ReadApplicationState` reads tabs, panes, modes, and session state.
- `ChangeApplicationState` switches tabs after a click or wheel event.

Focus the permission prompt and press `y` to grant both. Zellij stores the decision in its plugin permissions cache.

## Configure the sidebar

Reference the installed WASM from a layout pane:

```kdl
pane size=32 borderless=true {
    plugin location="file:~/.config/zellij/plugins/zellij-tabbar.wasm" {
        format "{index}:{name}"
        format_active "#[bg=236,fg=252,bold,fill]{index}:{name}*"
        max_name_length 26
        border "#[fg=dim]│"
        overflow_above "  ^ +{count}"
        overflow_below "  v +{count}"
    }
}
```

Common settings:

| Setting | Default | Purpose |
| --- | --- | --- |
| `format` | `{index}:{name}` | Inactive tab row |
| `format_active` | `{index}:{name} {indicators}` | Active tab row |
| `max_name_length` | `20` | Maximum display width for variables without an explicit width |
| `start_index` | `1` | First displayed tab number |
| `padding_top` | `0` | Empty rows above the list |
| `border` | empty | Right-side border text |
| `overflow_above` | `  ^ +{count}` | Hidden-tab row above the viewport |
| `overflow_below` | `  v +{count}` | Hidden-tab row below the viewport |
| `indicator_active` | `*` | Active-tab marker |
| `indicator_fullscreen` | `Z` | Fullscreen marker |
| `indicator_sync` | `S` | Synchronized-panes marker |

Formats support `{index}`, `{name}`, `{title}`, `{indicators}`, `{fullscreen}`, `{sync}`, and `{active}`. Use `{=12:title}` to limit one variable to 12 display columns. Inline styles use `#[fg=...]`, `#[bg=...]`, `bold`, `dim`, and `fill`. Colors may be named, an 8-bit index, `#RGB`, `#RRGGBB`, or `rgb(r,g,b)`.

Click a tab row to switch to it. Click an overflow row to move toward hidden tabs. Scroll up or down over the sidebar to move one tab at a time.

## Build and test

Install the Rust `wasm32-wasip1` target, then run:

```sh
cargo fmt --check
cargo test --locked --workspace --lib
cargo clippy --locked --workspace --lib
cargo clippy --locked --bin zellij-tabbar
cargo build --locked --release --target wasm32-wasip1
```

The release artifact is `target/wasm32-wasip1/release/zellij-tabbar.wasm`.

## Source and license

This project is adapted from [cfal/zellij-vertical-tabs](https://github.com/cfal/zellij-vertical-tabs) at commit `9b500a48427eed90654e5a226eae84908678ca92`. Alex Lau's MIT copyright and license are preserved in `LICENSE`; `NOTICE` records the source baseline.
