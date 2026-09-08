# zellij-tabbar

A clickable vertical tab bar for Zellij. Use it on its own for readable tab names and mouse navigation, or add pi integration for working spinners and unseen-completion markers across agent tabs.

This project is based on [cfal/zellij-vertical-tabs](https://github.com/cfal/zellij-vertical-tabs), with a focus on the pi workflow and optional activity rows. **Neither pi nor a pi extension is required to use the sidebar.**

## Use it without pi

Install the sidebar and load its layout. It displays your Zellij tab names, with the focused terminal pane's title as a fallback for default names:

```text
1:editor*
2:tests
3:logs
```

Click a row to switch tabs, or scroll over the sidebar to move one tab at a time. Each tab gets its own row; overflow indicators show how many tabs are hidden. Labels, colors, borders, and width limits are configurable. Existing Zellij keyboard bindings still work.

You can also run pi in these tabs without installing anything else. The sidebar will show ordinary tab names, but it will not detect when pi starts or finishes work.

## Add pi status

The optional [zellij-pi-tab-status](https://github.com/pbjorklund/zellij-pi-tab-status) extension gives pi's owning tab a worktree or directory name and updates it with agent status. For example:

```text
1:⠹ app:fix-login
2:● app:add-search
3:app:main*
```

- The spinner means pi or a tracked subagent is working.
- `●` means a background run has finished and you have not viewed its tab yet. Viewing the tab clears the marker.
- `*` marks the active Zellij tab in the supplied layout, not agent completion.

The extension waits for pi's full run to settle before marking it complete, rather than treating a pause between retries or queued follow-ups as done. It restores the base name when you view a completed tab or exit pi.

### Why two separate tools?

They run in different hosts and have different jobs:

| Tool | Runs inside | Responsibility |
| --- | --- | --- |
| `zellij-tabbar` | Zellij, as a WASM plugin | Displays tabs, handles mouse navigation, and renders optional activity rows |
| `zellij-pi-tab-status` | pi, as an extension | Reads pi lifecycle events and renames the owning Zellij tab to reflect agent state |

The Zellij plugin can read tab names, but pi's lifecycle events come from inside pi. The extension supplies that information through ordinary Zellij tab renames, so neither tool depends on the other: this sidebar works without pi, and the status extension also works with Zellij's built-in tab bar.

Extra todo and subagent rows use the separate [activity pipe](#activity-rows). Any compatible producer can send them, with or without pi. The status extension only renames tabs; it does not send activity rows.

## Install the sidebar

Zellij is the only runtime requirement for standalone use. This repository's live smoke test was tested with Zellij 0.45.0; the plugin builds against `zellij-tile` 0.44.3.

Download [v0.1.4](https://github.com/pbjorklund/zellij-tabbar/releases/tag/v0.1.4), verify its checksum, and install the WASM:

```sh
(
  set -eu
  download_dir=$(mktemp -d)
  cd "$download_dir"
  release=https://github.com/pbjorklund/zellij-tabbar/releases/download/v0.1.4
  curl -fLO "$release/zellij-tabbar.wasm"
  curl -fLO "$release/SHA256SUMS"
  sha256sum --check SHA256SUMS
  mkdir -p ~/.config/zellij/plugins
  install -m 0644 zellij-tabbar.wasm ~/.config/zellij/plugins/zellij-tabbar.wasm
)
```

On macOS, replace `sha256sum --check` with `shasum -a 256 --check`.

From a checkout of this repository, copy the example layout and start a new session:

```sh
mkdir -p ~/.config/zellij/layouts
cp examples/vertical-tabs-left.kdl ~/.config/zellij/layouts/
zellij --session sidebar-demo --layout vertical-tabs-left
```

Use an unused session name. Attaching to an existing session does not replace its layout. If you use `XDG_CONFIG_HOME` or another config directory, adjust the install paths and the layout's plugin URL together.

At the permission prompt, focus the plugin and press `y`. It requests:

- `ReadApplicationState` to read tabs, panes, modes, and session state.
- `ChangeApplicationState` to switch tabs on clicks and wheel events.

Zellij caches approval. After approval, the sidebar becomes non-selectable so ordinary pane focus stays on terminal panes; mouse navigation still works.

The sidebar is ready to use. No pi package or activity producer is needed.

### Layout choices

To use the sidebar for new sessions, add this to your existing Zellij `config.kdl` without replacing other settings:

```kdl
default_layout "vertical-tabs-left"
```

To open another tab with the sidebar inside a session:

```sh
zellij action new-tab --layout vertical-tabs-left
```

The [main layout](examples/vertical-tabs-left.kdl) uses 32 columns. The optional [swap layout](examples/vertical-tabs-left.swap.kdl) uses 36 columns and provides tiled and floating arrangements; copy it beside the main layout to use it. For narrow terminals, [horizontal-tabs.kdl](examples/horizontal-tabs.kdl) uses Zellij's built-in horizontal tab bar instead.

**DevTools-managed machines:** use the normal `mise run setup` workflow in `ProVessla.DevTools` instead of installing a second copy. DevTools owns the installed WASM and team layouts; the Omarchy setup delegates shared installation to it. A local build in this checkout does not update that installation, and setup can replace a manually installed build with a published release.

## Install pi status (optional)

Skip this section if you only want the sidebar. For automatic agent markers, you need pi 0.84.3 or newer and Zellij's `list-panes`, `list-tabs`, and `rename-tab-by-id` actions.

Install the companion package:

```sh
pi install git:github.com/pbjorklund/zellij-pi-tab-status
```

Start a new pi TUI inside a Zellij terminal pane. The extension runs automatically there and does nothing outside Zellij. As with any pi extension, review its source before installing: it runs with your user permissions.

To try the workflow, start a task in pi, then switch to another tab before it finishes. The first tab should show a spinner while the run is active, then `●` once it settles. Return to that tab to clear the completion marker.

## Configure the sidebar

Set options in the layout's plugin pane. These settings work with or without pi. `{name}` displays the Zellij tab name, including any markers supplied by the optional status extension:

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

| Setting | Default | Purpose |
| --- | --- | --- |
| `format` | `{index}:{name}` | Inactive tab row |
| `format_active` | `{index}:{name} {indicators}` | Active tab row |
| `max_name_length` | `20` | Display-column limit for variables without an explicit width |
| `start_index` | `1` | First displayed tab number; does not change navigation targets |
| `padding_top` | `0` | Empty rows above the list |
| `border` | empty | Right border text; `border_char` is a fallback alias |
| `overflow_above` | `  ^ +{count}` | Hidden-tab count above the viewport |
| `overflow_below` | `  v +{count}` | Hidden-tab count below the viewport |
| `indicator_active` | `*` | Active-tab marker |
| `indicator_fullscreen` | `Z` | Fullscreen marker |
| `indicator_sync` | `S` | Synchronized-panes marker |
| `activity_format` | `#[fg=dim]{activity}` | Style wrapper for activity rows |
| `diagnostics` | `false` | Set to `"true"` for sampled diagnostics in Zellij's log |

Formats accept `{index}`, `{name}`, `{title}`, `{indicators}`, `{fullscreen}`, `{sync}`, and `{active}`. Use `{=12:title}` to limit a variable to 12 display columns. Default tab names fall back to the focused non-plugin pane's title; placeholder titles fall back to `...`.

Inline styles use `#[fg=...,bg=...,bold,dim,fill]`. Colors can be names, 8-bit indices, `#RGB`, `#RRGGBB`, or `rgb(r,g,b)`. `fill` extends the active row's background through its padding, but not its border. Raw terminal control characters in labels and activity text become spaces so each item stays on one row.

When tabs overflow, the sidebar keeps the active tab in view when there is room for tab rows. Click an overflow row to move toward hidden tabs. Activity rows link to their parent tab; empty rows do not switch tabs. Existing Zellij keyboard bindings are unchanged.

## Activity rows

An activity producer can add todo or subagent rows beneath a tab. This is an optional API, not part of the companion status extension's tab-renaming workflow.

For a quick test inside Zellij, replace `worktree-name` with the matching focused-pane title, or tab name if no usable pane title exists:

```sh
zellij action pipe --name activity -- \
  "{\"zsession\":\"$ZELLIJ_SESSION_NAME\",\"name\":\"worktree-name\",\"todos\":[{\"status\":\"in_progress\",\"text\":\"Run tests\"}]}"
```

This broadcasts to running plugins; it does not launch another plugin instance. Use a JSON serializer when sending arbitrary names or text.

| Field | Meaning |
| --- | --- |
| `zsession` | Zellij session name; must match the sidebar's session |
| `name` | Required lookup name. The sidebar normalizes the focused-pane title, or the tab name when no usable pane title exists |
| `todos` | Array of `{ "status": "in_progress", "text": "Run tests" }`; statuses are `pending`, `in_progress`, and `done` |
| `subagents` | Object keyed by producer-chosen IDs; each entry has optional `icon`, `glyph`, and `title` strings |

Name normalization trims leading whitespace and removes one leading non-alphanumeric status character plus following whitespace. Send the normalized name as the key.

Nonempty subagents take precedence over todos. Done todos are hidden; unfinished todos show `☐` or `▣`, capped at six items plus an overflow row. Extra rows use spare height, leaving room for remaining visible tabs and the below-overflow indicator.

Each message replaces the entry for its session/name pair. Send empty `todos` and `subagents` to clear it. Entries have no expiry timer.

The plugin also accepts `set_selectable` with payload `true` or `false`, and `toggle_selectable`, for integrations that need to change pane selectability.

## Troubleshooting

### Sidebar missing or still running an old build

A WASM file on disk does not prove a session has loaded it. Inside the affected session, inspect the running panes and layout:

```sh
zellij action list-panes --all --json
zellij action dump-layout
sha256sum ~/.config/zellij/plugins/zellij-tabbar.wasm
```

Look for `is_plugin: true`, the `zellij-tabbar.wasm` URL, and `exited: false`. Use `--all` because the sidebar is non-selectable. From outside the session, add `--session <name>` before `action`.

Check permissions, the layout's plugin URL, and `zellij setup --check`. A `tab-bar` alias alone does not load the plugin; the layout must contain its pane. After replacing the WASM or editing plugin options, start a new session or explicitly reload the plugin. Existing instances do not pick up those changes automatically.

### Tabs show names but no pi status

Ordinary names are expected in standalone use. If you want automatic pi markers, follow [Install pi status](#install-pi-status-optional), then check that pi is running in its TUI inside Zellij. Keep `{name}` in both sidebar formats: `{title}` reads the focused-pane title instead. The activity pipe is not needed for tab status.

### Sidebar stops updating

Set `diagnostics "true"` in the plugin configuration, then start a separate session or reload the plugin. For managed layouts, edit the owning template rather than a generated file.

On Linux, diagnostics normally appear in:

```sh
log="${TMPDIR:-/tmp}/zellij-$(id -u)/zellij-log/zellij.log"
grep 'zellij-tabbar version=' "$log"
```

Use the server's startup `TMPDIR` if it differs from your shell's. Logs include plugin version and ID, event counts, permission state, queue size, tab IDs and positions, and render dimensions. They omit titles, session names, commands, configuration values, and activity payloads.

Look for `permission_wait`, `permission_denied`, `active_marker_missing`, `own_pane_missing`, `empty_tabs`, or `invalid_activity`. `refresh_suppressed` is expected for a background sidebar. Events are sampled on occurrences 1, 2, 4, 8, and so on; this is not a complete trace. A host stall can prevent callbacks and new log lines.

When reporting a problem, include the Zellij version, installed WASM checksum, relevant diagnostics, and typed pane ID. `plugin_4` and `terminal_4` are different panes; stable tab IDs also differ from tab positions. Render logs alone do not prove the terminal displayed a frame.

## Development

Use Rust 1.88 or newer. From the repository root, run the CI checks:

```sh
rustup target add wasm32-wasip1
cargo fmt --check
cargo test --locked --workspace --lib
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts -p 'test_*.py'
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo clippy --locked --target wasm32-wasip1 --bin zellij-tabbar -- -D warnings
cargo build --locked --release --target wasm32-wasip1
```

The artifact is `target/wasm32-wasip1/release/zellij-tabbar.wasm`. It runs inside Zellij, not through host `cargo run`. To try it without replacing an installed release, use a separate filename and a separate layout pointing to that file.

`src/plugin.rs` owns callbacks and state; its `config`, `formatting`, and `rendering` modules handle configuration, styled lines, and immutable frame construction. Library tests cover navigation and display-width helpers, activity parsing, and real plugin callback sequences through a recording host. Python tests reject incorrect smoke output and failed cleanup checks without launching Zellij. The WASI Clippy check covers the Zellij host adapter.

For a live check after building:

```sh
python3 scripts/smoke-zellij.py
```

The smoke test creates its own disposable PTY session and checks sidebar output through tab switches, renames, movement, closure, resizing, and mouse clicks. It does not replace your installed plugin or stop existing sessions. Use `--wasm <path>` to select another artifact or `--timeout <seconds>` to change the default 20-second assertion timeout. It tests the sidebar, not the companion pi extension or an intermittent hang in an existing session.

Run `cargo bench --locked --bench plugin` for host rendering and permission-replay benchmarks. Activity cases cover 1,000 subagents and a 1MB title in a three-row viewport, to catch work on text and rows that cannot fit. Compare timings on the same machine; they exclude Zellij's WASM execution and terminal output.

## Source and license

Adapted from Alex Lau's [zellij-vertical-tabs](https://github.com/cfal/zellij-vertical-tabs) at commit `9b500a48427eed90654e5a226eae84908678ca92`. The upstream MIT copyright and license are preserved in [LICENSE](LICENSE); [NOTICE](NOTICE) records the source baseline.
