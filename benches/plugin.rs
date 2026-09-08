use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::{Duration, Instant};
use zellij_tabbar::plugin::{Host, Tabbar};
use zellij_tile::prelude::*;

#[derive(Default)]
struct BenchHost;

impl Host for BenchHost {
    fn plugin_id(&mut self) -> u32 {
        4
    }
    fn subscribe(&mut self, _: &[EventType]) {}
    fn request_permissions(&mut self, _: &[PermissionType]) {}
    fn set_selectable(&mut self, _: bool) {}
    fn switch_tab(&mut self, _: u32) {}
    fn render(&mut self, frame: &str) {
        black_box(frame);
    }
}

fn loaded() -> Tabbar<BenchHost> {
    let mut state = Tabbar::default();
    state.load(BTreeMap::from([
        (
            "format_active".into(),
            "#[bg=236,fg=252,bold,fill]{index}:{name}*".into(),
        ),
        ("border".into(), "#[fg=dim]│".into()),
    ]));
    state
}

fn render_sample() -> Duration {
    let mut state = loaded();
    state.update(Event::PermissionRequestResult(PermissionStatus::Granted));
    state.update(Event::TabUpdate(
        (0..32)
            .map(|i| TabInfo {
                tab_id: i + 10,
                position: i,
                active: i == 15,
                name: format!("worktree-{i}-界-long-title"),
                ..TabInfo::default()
            })
            .collect(),
    ));
    let start = Instant::now();
    for _ in 0..2_000 {
        state.render(40, 32);
    }
    start.elapsed()
}

fn replay_sample() -> Duration {
    let mut state = loaded();
    for _ in 0..4_000 {
        state.update(Event::ModeUpdate(ModeInfo::default()));
    }
    let start = Instant::now();
    black_box(state.update(Event::PermissionRequestResult(PermissionStatus::Granted)));
    start.elapsed()
}

fn main() {
    for (name, sample) in [
        (
            "render 2000 frames (32 tabs, 40x32)",
            render_sample as fn() -> Duration,
        ),
        (
            "replay 4000 queued events",
            replay_sample as fn() -> Duration,
        ),
    ] {
        sample();
        let mut samples: Vec<_> = (0..5).map(|_| sample()).collect();
        samples.sort();
        println!("{name}: median {:?} (5 samples)", samples[2]);
    }
}
