# Repository guide

`zellij-tabbar` is a Rust/WASI plugin that renders Zellij tabs in a vertical sidebar, with mouse navigation and optional activity rows.

## Code map

- `src/main.rs`: WASM plugin registration and the `ZellijHost` adapter. Native execution reports that the plugin must run inside Zellij.
- `src/plugin.rs`: `Tabbar<H>` event state, configuration, format/color parsing, and frame rendering. All Zellij calls and frame output go through `Host`.
- `src/plugin/event_tests.rs`: real callback sequences, recorded host effects, frame output, and rendered-row click targets.
- `src/lib.rs`: pure helpers for visible ranges, navigation, active-tab identity, render gating, and Unicode display-width truncation. Unit tests are in the same file.
- `benches/plugin.rs`: dependency-free rendering and permission-replay benchmarks.
- `scripts/smoke-zellij.py`: isolated live PTY test for sidebar output and mouse navigation, using the built WASM.
- `activity/src/lib.rs`: JSON activity parsing and text rows for subagents and todos, with inline unit tests. This workspace crate has no Zellij dependency.
- `examples/`: KDL layouts for the sidebar, swap layouts, and a fallback that uses Zellij's built-in horizontal tab bar.
- `.github/workflows/ci.yml`: formatting, host tests, Clippy, WASI builds, and tagged releases.
- `README.md`: installation, permissions, configuration, and build instructions.

## Build and validation

Use Cargo from the repository root. The root package uses Rust edition 2024 and requires Rust 1.88 or newer; `activity` uses edition 2021. `zellij-tile` is pinned to `=0.44.3`.

Install the WASI target if missing:

```sh
rustup target add wasm32-wasip1
```

Run the checks used by CI before handing off code changes:

```sh
cargo fmt --check
cargo test --locked --workspace --lib
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo clippy --locked --target wasm32-wasip1 --bin zellij-tabbar -- -D warnings
cargo build --locked --release --target wasm32-wasip1
```

Start with a focused library test, such as `cargo test --locked --lib plugin::event_tests` or `cargo test --locked -p activity --lib`. The library suite includes the plugin callbacks and renderer through a recording host, without WASM linker stubs. Use `cargo bench --locked --bench plugin` for performance comparisons; keep timing thresholds out of unit tests.

The plugin artifact is `target/wasm32-wasip1/release/zellij-tabbar.wasm`. It runs inside Zellij, not through host `cargo run`. For UI or event changes, run `python3 scripts/smoke-zellij.py` after the WASI build (tested with Zellij 0.45.0). It checks output and mouse clicks in its own disposable session without changing the installed plugin. Keep existing user sessions intact during diagnosis. Check overflow, wheel navigation, Unicode labels, and affected activity rows separately when relevant; report manual and automated checks separately.

## Change rules

- Keep Zellij API calls in `ZellijHost` and exercise `Tabbar` through a recording host in tests. Test callback sequences as well as isolated helpers.
- Compile static label formats and borders during configuration, then reuse them during rendering. Replay queued permission events in FIFO order without repeatedly shifting the queue.
- Use terminal display widths, not byte lengths, for sidebar sizing. Preserve ANSI resets, active-row fill, and border placement. The activity crate currently truncates by character count; do not assume it uses the sidebar's display-width logic.
- Keep `row_targets` aligned with the rows actually rendered. Activity rows target their parent tab; padding and empty rows have no target. Runtime clicks use this map, not the standalone `tab_target_at_row` helper.
- Keep tab IDs, tab positions, zero-based vector indices, and one-based navigation targets distinct. `start_index` changes displayed labels, not navigation targets.
- Keep the effective active tab consistent across render gating, labels, and navigation when an update lacks an active marker. Clear cached own-tab position when the pane manifest omits the plugin, so the unknown-location fallback can refresh the sidebar.
- Test tab movement and closure with both pane/tab event orders and with stable tab IDs different from positions. A complete snapshot pair normally repairs ordering differences; distinguish reproduced edge cases from a confirmed live hang.
- Subscribe before requesting permissions. Queue early events until permissions are granted, then replay them.
- Keep diagnostics opt-in through `diagnostics "true"` and send them through `Host::log` to stderr, never frame output. Sample repeated events and log numeric state only, not titles, session names, commands, configuration values, or activity payloads.
- Activity pipe payloads are parsed by `activity::parse_activity`. Entries are keyed by Zellij session and name; rendering looks them up using the current session and normalized focused-pane title, with a tab-name fallback. Preserve this matching when changing activity handling.
- Subagents take precedence over todos. Done todos are hidden; todo output is capped at six items plus an overflow row. Activity rows must leave room for the remaining visible tabs and the below-overflow row.
- When adding or changing configuration, update `StyleConfig`, `Tabbar::load`, `README.md`, and relevant KDL examples together. Preserve existing keys and aliases unless the task explicitly changes compatibility.
- Keep `Cargo.lock` tracked. Do not change the pinned Zellij API version or add dependencies without a task-specific reason.
- Preserve upstream attribution in `LICENSE`, `NOTICE`, and source headers. Do not commit build outputs from `target/` or `dist/`.

## Local code intelligence

When CodeGraph is available:

- Check `codegraph status --json .`. Before creating an index, confirm `.codegraph/` is ignored or otherwise safe local cache state, then use `codegraph init -i .`.
- For vague code-location questions, start with `codegraph explore "<question>"` or `codegraph query "<symbol>" --limit 20`, then read the returned source.
- Before editing shared symbols, run `codegraph impact "<symbol>" --depth 2` and inspect relevant callers.
- After edits, run `codegraph sync .` if the index exists. Keep the index out of commits.
- Treat empty graph results as leads, not proof. Confirm behavior in source and tests, especially Zellij callbacks registered through macros.
