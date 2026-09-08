# zellij-tabbar

`zellij-tabbar` puts Zellij tabs in a left sidebar, with clickable rows, configurable labels, and room for worktree names and agent status.

## Why a sidebar?

A horizontal tab bar shares one terminal row between every tab. This plugin gives each visible tab its own row, so longer names can use a fixed sidebar width. It shows hidden-tab counts when the list overflows and keeps the active tab in the viewport when there is room for tab rows.

The trade-off is terminal width: the supplied layout uses 32 columns. For narrow terminals, [horizontal-tabs.kdl](examples/horizontal-tabs.kdl) uses Zellij's built-in tab bar instead, without this plugin.

The sidebar reads Zellij's tab names and focused-pane titles. It does not create worktrees, run agents, or detect their status itself. A separate integration can rename tabs to show a spinner or completion marker; this plugin displays those names. It also accepts optional activity messages for extra rows below a tab.

## Where it is used

The ProVessla.DevTools baseline and the maintainer's Omarchy configuration use this plugin for their left-hand tab sidebar.

| Owner | Responsibility |
| --- | --- |
| This repository | Rust source, WASM releases, standalone example layouts |
| `ProVessla.DevTools/scripts/common/install-zellij.sh` | Installs a checksum-verified release and renders the team layouts from `templates/zellij/` |
| `omarchy-pbjorklund/dotfiles-overrides/.config/zellij/config.kdl` | Personal settings and keybindings; selects `provessla-vertical-tabs-left` for the default layout and new tabs |
| `zellij-pi-tab-status` | Separate PI extension that updates Zellij tab names with agent status |

DevTools installs `zellij-tabbar.wasm` under `~/.config/zellij/plugins/` and owns `provessla-vertical-tabs-left.kdl` and its `.swap.kdl` companion under `~/.config/zellij/layouts/`. Its installer honors `XDG_CONFIG_HOME`, refreshes those layouts, and preserves an existing `config.kdl`; ordered migrations set the team default.

Omarchy's `install-scripts/bootstrap-provessla-devtools.sh` delegates shared tool setup to DevTools. The live personal `config.kdl` is a symlink into the Omarchy repo, while the plugin and team layouts are installed files.

For a DevTools-managed machine, use its normal `mise run setup` workflow rather than maintaining a second plugin installer here. Changes to the WASM in this checkout do **not** update that baseline: publish a release first. The updated DevTools installer follows the latest published release and checks its checksum. Older DevTools checkouts may still pin an earlier version, so update that checkout before setup. Setup can replace a locally installed build with the selected release.

## Standalone installation

Install Zellij first. The source uses `zellij-tile` 0.44.3; the installed v0.1.2 plugin was confirmed running under Zellij 0.45.0 during the consumer check. This is not a compatibility guarantee for every Zellij version.

Download the release and its checksum into a fresh directory:

```sh
download_dir=$(mktemp -d)
cd "$download_dir"
release=https://github.com/pbjorklund/zellij-tabbar/releases/download/v0.1.3
curl -fLO "$release/zellij-tabbar.wasm"
curl -fLO "$release/SHA256SUMS"
sha256sum --check SHA256SUMS
```

On macOS, use `shasum -a 256 --check SHA256SUMS` instead. Continue only if verification succeeds:

```sh
mkdir -p ~/.config/zellij/plugins
install -m 0644 zellij-tabbar.wasm ~/.config/zellij/plugins/zellij-tabbar.wasm
```

From this repository's root, copy the sidebar layout and start a new session:

```sh
mkdir -p ~/.config/zellij/layouts
cp examples/vertical-tabs-left.kdl ~/.config/zellij/layouts/
zellij --session sidebar-demo --layout vertical-tabs-left
```

Use an unused session name. Attaching to an existing session does not replace its layout. These examples use `~/.config/zellij`; if you use another config directory, adjust both the install paths and the layout's plugin URL.

To make the sidebar the default for new sessions, set this in your existing `config.kdl` without replacing other settings:

```kdl
default_layout "vertical-tabs-left"
```

For a new tab inside an existing session:

```sh
zellij action new-tab --layout vertical-tabs-left
```

The optional [vertical-tabs-left.swap.kdl](examples/vertical-tabs-left.swap.kdl) supplies tiled and floating swap layouts. Copy it beside the main layout to use it; its sidebar is 36 columns wide, rather than the main example's 32.

## Permissions and navigation

The plugin requests two permissions on first use:

- `ReadApplicationState` reads tabs, panes, modes, and session state.
- `ChangeApplicationState` switches tabs after a click or wheel event.

Focus the permission prompt and press `y` to grant both. Zellij caches the decision; installers should not preapprove it.

Click a tab row to switch to it. Click an overflow row to move toward hidden tabs. Scroll over the sidebar to move one tab at a time. Activity rows, when present, switch to their parent tab. Existing Zellij keyboard bindings still work; the plugin does not install keybindings.

## Configure the sidebar

Set plugin options in the layout pane:

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
| `max_name_length` | `20` | Display-width limit for variables without an explicit width |
| `start_index` | `1` | First displayed tab number, without changing navigation targets |
| `padding_top` | `0` | Empty rows above the list |
| `border` | empty | Right-side border text; `border_char` is a fallback alias |
| `overflow_above` | `  ^ +{count}` | Hidden-tab row above the viewport |
| `overflow_below` | `  v +{count}` | Hidden-tab row below the viewport |
| `indicator_active` | `*` | Active-tab marker |
| `indicator_fullscreen` | `Z` | Fullscreen marker |
| `indicator_sync` | `S` | Synchronized-panes marker |
| `activity_format` | `#[fg=dim]{activity}` | Style wrapper for optional activity rows |
| `diagnostics` | `false` | Set to `"true"` to write sampled state diagnostics to Zellij's log |

Formats support `{index}`, `{name}`, `{title}`, `{indicators}`, `{fullscreen}`, `{sync}`, and `{active}`. Use `{=12:title}` to limit one variable to 12 display columns. Default tab names fall back to the focused non-plugin pane's title; placeholder titles fall back to `...`.

Inline styles use `#[fg=...,bg=...,bold,dim,fill]`. Use named colors, an 8-bit index, `#RGB`, or `#RRGGBB`. `fill` extends the active row's background through its padding, but not its border. Prefer hex for inline RGB colors: the current style parser splits on commas, so `rgb(r,g,b)` does not work reliably inside a style directive.

## Optional activity rows

Normal PI status in the DevTools setup comes from **tab renaming**, not this API. No activity producer is required for the sidebar to work.

A producer can send JSON to the `activity` pipe. For example, from inside Zellij, replacing `worktree-name` with the matching name:

```sh
zellij action pipe --name activity -- \
  "{\"zsession\":\"$ZELLIJ_SESSION_NAME\",\"name\":\"worktree-name\",\"todos\":[{\"status\":\"in_progress\",\"text\":\"Run tests\"}]}"
```

This broadcasts to running plugins; it does not select or launch another plugin instance. Producers should use a JSON serializer for arbitrary names and text.

- `name` is required. Match the focused-pane title, or the tab name when no usable pane title exists. Matching trims leading whitespace and removes one leading non-alphanumeric status character plus following whitespace.
- `zsession` must match the Zellij session for the entry to appear there.
- `todos` accepts `pending`, `in_progress`, and `done` statuses. Done items are hidden; at most six unfinished items plus an overflow row are shown.
- `subagents` is an object keyed by producer-chosen IDs, with optional `icon`, `glyph`, and `title` fields. Nonempty subagents take precedence over todos.
- Each message replaces the entry for that session/name pair. Send empty `todos` and `subagents` to clear it. Entries have no expiry timer.
- Extra rows use spare vertical space; tab rows and the below-overflow indicator take priority.

The plugin also accepts `set_selectable` with payload `true` or `false`, and `toggle_selectable`. It becomes non-selectable after permission approval so ordinary pane focus can stay on terminal panes.

## Verify what is actually running

A file in the plugins directory is not proof that a session uses it. With a recent Zellij version, run these read-only checks inside the session:

```sh
zellij action list-panes --all --json
zellij action dump-layout
sha256sum ~/.config/zellij/plugins/zellij-tabbar.wasm
```

Look for `is_plugin: true`, the `zellij-tabbar.wasm` plugin URL, and `exited: false`. Use `--all`: the default pane listing can omit this non-selectable sidebar. From outside the session, add `--session <name>` before `action`.

If the sidebar is missing, check the active session's layout, the plugin URL, permissions, and `zellij setup --check`. A `tab-bar` alias in `config.kdl` alone does not load a tab bar; the layout determines which plugin pane is used.

After replacing a WASM file, test in a new session or explicitly reload the plugin. Building this repo does not replace the installed file or reload an existing instance. A different local-build checksum does not by itself prove a source-version difference.

## Diagnose a stuck sidebar

Set `diagnostics "true"` inside the sidebar's plugin configuration, then start a separate session or explicitly reload that plugin. Existing instances do not pick up configuration edits automatically. For managed layouts, change the owning template rather than the generated live file.

Diagnostics go to stderr, which Zellij records in its log, not in the sidebar. On Linux, check the temp directory used by the Zellij server when it started:

```sh
log="${TMPDIR:-/tmp}/zellij-$(id -u)/zellij-log/zellij.log"
grep 'zellij-tabbar version=' "$log"
```

If your shell's `TMPDIR` differs from the server's, adjust that path. Each line includes the plugin version and ID, event, occurrence count, permission state, queued-event count, tab count, active stable tab ID, active and own tab positions, and last render dimensions. Zellij adds timestamps. Keep the installed WASM checksum alongside the log when reporting a problem, since local builds can share a version number.

Look for `permission_wait` or `permission_denied`, `active_marker_missing`, `own_pane_missing`, `empty_tabs`, and `invalid_activity`. `refresh_suppressed` means a changed snapshot belongs to a background sidebar. `render_skipped` means permissions or tabs are missing. `render_start` and `render` bracket frame generation and output, but do not prove that the terminal displayed it.

Each event is logged on occurrences 1, 2, 4, 8, and so on, per plugin instance. This limits repeated messages during an event flood; it is not a complete event trace. Diagnostics omit session names, tab/pane titles, commands, configuration values, and activity payloads. They are off by default. A host stall can prevent callbacks and therefore prevent new diagnostic lines too.

## Build and test

Use Rust 1.88 or newer and install the WASI target:

```sh
rustup target add wasm32-wasip1
cargo fmt --check
cargo test --locked --workspace --lib
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo clippy --locked --target wasm32-wasip1 --bin zellij-tabbar -- -D warnings
cargo build --locked --release --target wasm32-wasip1
```

The artifact is `target/wasm32-wasip1/release/zellij-tabbar.wasm`. The library tests cover the helper library, the `activity` crate, and the plugin's real event handlers and renderer through a recording host. They include permission replay, missing active markers, incomplete pane snapshots, tab closure in both event orders, activity rows, click targets, and narrow borders. `src/main.rs` contains the WASM host adapter; WASI Clippy checks that adapter separately.

Run the repeatable host microbenchmarks with `cargo bench --locked --bench plugin`. They report median times for 2,000 rendered frames and replaying 4,000 queued permission events. Compare before and after on the same machine; these timings exclude Zellij's WASM execution and terminal output.

For a live check after building, run `python3 scripts/smoke-zellij.py` (tested with Zellij 0.45.0). It creates a disposable PTY session with temporary config, cache, and data directories, grants that session's plugin permissions, then checks actual sidebar output through switches, renames, movement, closure, resizing, and mouse clicks. It stops only its own session and does not replace the installed plugin. Each assertion has a 20-second timeout; use `--timeout` to change it or `--wasm` to test another artifact.

This smoke test covers ordinary event sequences, not an intermittent hang in an existing session. To investigate a live incident, record the Zellij version and typed pane ID: `plugin_4` and `terminal_4` are different panes. A stable tab ID is also different from its current tab position.

To try a local build manually without overwriting a managed release, use a separate filename and point a separate layout at that file.

## Source and license

This project is adapted from [cfal/zellij-vertical-tabs](https://github.com/cfal/zellij-vertical-tabs) at commit `9b500a48427eed90654e5a226eae84908678ca92`. Alex Lau's MIT copyright and license are preserved in [LICENSE](LICENSE); [NOTICE](NOTICE) records the source baseline.
