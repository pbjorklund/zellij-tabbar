use super::*;

#[derive(Default)]
struct RecordingHost {
    calls: Vec<&'static str>,
    switches: Vec<u32>,
    frames: Vec<String>,
    logs: Vec<String>,
}

impl Host for RecordingHost {
    fn plugin_id(&mut self) -> u32 {
        4
    }
    fn subscribe(&mut self, events: &[EventType]) {
        assert!(events.contains(&EventType::PermissionRequestResult));
        self.calls.push("subscribe");
    }
    fn request_permissions(&mut self, permissions: &[PermissionType]) {
        assert_eq!(
            permissions,
            [
                PermissionType::ReadApplicationState,
                PermissionType::ChangeApplicationState
            ]
        );
        self.calls.push("permissions");
    }
    fn set_selectable(&mut self, selectable: bool) {
        self.calls.push(if selectable {
            "selectable"
        } else {
            "non-selectable"
        });
    }
    fn switch_tab(&mut self, index: u32) {
        self.switches.push(index);
    }
    fn render(&mut self, frame: &str) {
        self.frames.push(frame.to_owned());
    }
    fn log(&mut self, message: &str) {
        self.logs.push(message.to_owned());
    }
}

type State = Tabbar<RecordingHost>;

fn tab(id: usize, position: usize, active: bool) -> TabInfo {
    TabInfo {
        tab_id: id,
        position,
        active,
        name: format!("work-{id}"),
        ..TabInfo::default()
    }
}

fn manifest(position: usize) -> PaneManifest {
    PaneManifest {
        panes: [(
            position,
            vec![
                PaneInfo {
                    id: 4,
                    is_plugin: true,
                    ..PaneInfo::default()
                },
                PaneInfo {
                    id: 4,
                    is_focused: true,
                    title: "project".into(),
                    ..PaneInfo::default()
                },
            ],
        )]
        .into(),
    }
}

fn state() -> State {
    let mut state = State::default();
    state.load(BTreeMap::new());
    state.update(Event::PermissionRequestResult(PermissionStatus::Granted));
    state
}

#[test]
fn missing_active_marker_keeps_the_selected_tab_renderable_and_styled() {
    let mut state = state();
    state.update(Event::PaneUpdate(manifest(1)));
    state.update(Event::TabUpdate(vec![tab(10, 0, false), tab(20, 1, true)]));
    let mut changed = tab(20, 1, false);
    changed.name = "renamed".into();

    assert!(state.update(Event::TabUpdate(vec![tab(10, 0, false), changed])));
    state.render(3, 30);

    assert_eq!(state.active_tab_id, Some(20));
    assert!(state.host.frames.last().unwrap().contains("2:renamed *"));
}

#[test]
fn missing_own_pane_drops_stale_position_after_a_tab_closes() {
    let mut state = state();
    state.update(Event::PaneUpdate(manifest(1)));
    state.update(Event::TabUpdate(vec![
        tab(10, 0, false),
        tab(20, 1, true),
        tab(30, 2, false),
    ]));
    state.update(Event::TabUpdate(vec![tab(20, 0, true), tab(30, 1, false)]));

    assert!(state.update(Event::PaneUpdate(PaneManifest::default())));
    assert_eq!(state.own_tab_position, None);
    state.render(2, 30);
    assert!(state.host.frames.last().unwrap().contains("1:work-20 *"));
}

#[test]
fn tab_close_recovers_in_both_snapshot_orders_without_confusing_ids() {
    for panes_first in [false, true] {
        let mut state = state();
        state.update(Event::PaneUpdate(manifest(1)));
        state.update(Event::TabUpdate(vec![
            tab(10, 0, false),
            tab(20, 1, true),
            tab(30, 2, false),
        ]));
        let panes = Event::PaneUpdate(manifest(0));
        let tabs = Event::TabUpdate(vec![tab(20, 0, true), tab(30, 1, false)]);
        let events = if panes_first {
            [panes, tabs]
        } else {
            [tabs, panes]
        };
        for event in events {
            state.update(event);
        }
        assert!(state.own_tab_is_active());
        assert_eq!(state.own_tab_position, Some(0));
        state.render(2, 30);
        state.update(Event::Mouse(Mouse::LeftClick(1, 0)));
        assert_eq!(state.host.switches, [2]);
        assert_eq!(state.active_tab_id, Some(20));
    }
}

#[test]
fn inactive_snapshots_are_retained_and_rendered_on_reactivation() {
    let mut state = state();
    state.update(Event::PaneUpdate(manifest(1)));
    assert!(!state.update(Event::TabUpdate(vec![tab(10, 0, true), tab(20, 1, false)])));
    let mut renamed = tab(20, 1, false);
    renamed.name = "updated in background".into();
    assert!(!state.update(Event::TabUpdate(vec![tab(10, 0, true), renamed.clone()])));
    renamed.active = true;
    assert!(state.update(Event::TabUpdate(vec![tab(10, 0, false), renamed])));
    state.render(2, 32);
    assert!(
        state
            .host
            .frames
            .last()
            .unwrap()
            .contains("updated in backgr...")
    );
}

#[test]
fn loading_subscribes_before_requesting_permissions_and_replays_in_order() {
    let mut state = State::default();
    state.load(BTreeMap::new());
    assert_eq!(state.host.calls, ["subscribe", "permissions"]);
    assert!(!state.update(Event::PaneUpdate(manifest(0))));
    assert!(!state.update(Event::TabUpdate(vec![tab(10, 0, true)])));
    assert!(!state.update(Event::TabUpdate(vec![tab(20, 0, true)])));
    state.render(2, 20);
    assert!(state.host.frames.is_empty());
    assert!(state.update(Event::PermissionRequestResult(PermissionStatus::Granted)));
    assert!(state.pending_events.is_empty());
    assert_eq!(state.active_tab_id, Some(20));
    assert_eq!(state.host.calls.last(), Some(&"non-selectable"));
    state.render(2, 20);
    assert!(state.host.frames[0].contains("1:work-20 *"));
}

#[test]
fn empty_tabs_clear_click_targets_and_recover_on_next_snapshot() {
    let mut state = state();
    state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
    state.render(2, 20);
    state.update(Event::TabUpdate(vec![]));
    state.render(2, 20);
    state.update(Event::Mouse(Mouse::LeftClick(0, 0)));
    assert!(state.host.switches.is_empty());
    assert!(state.update(Event::TabUpdate(vec![tab(20, 0, true)])));
    state.render(2, 20);
    assert_eq!(state.row_targets, [Some(1), None]);
}

#[test]
fn rendering_keeps_activity_click_targets_and_reserves_primary_rows() {
    let mut state = state();
    state.style.padding_top = 1;
    state.style.start_index = 7;
    state.update(Event::TabUpdate(vec![
        tab(10, 0, true),
        tab(20, 1, false),
        tab(30, 2, false),
    ]));
    let (_, _, activity) = activity::parse_activity(
        r#"{"name":"work-10","todos":[{"text":"test"},{"text":"build"}]}"#,
    )
    .unwrap();
    state.activity.insert("\u{1}work-10".into(), activity);
    state.render(5, 30);
    assert_eq!(state.host.frames.last().unwrap().lines().count(), 5);
    assert_eq!(
        state.row_targets,
        [None, Some(1), Some(1), Some(2), Some(3)]
    );
    state.update(Event::Mouse(Mouse::LeftClick(2, 0)));
    assert_eq!(state.host.switches, [1]);
}

#[test]
fn border_never_wraps_when_the_sidebar_is_narrower_than_the_border() {
    let mut state = state();
    state.style.border = parse_styled_string("界");
    state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
    for cols in 0..=3 {
        state.render(2, cols);
        let frame = state.host.frames.last().unwrap().replace("\x1b[m", "");
        for line in frame.lines() {
            assert!(line.width() <= cols, "{cols} columns: {line:?}");
        }
    }
}

#[test]
fn overflow_rows_and_wheel_navigation_stay_in_bounds() {
    let mut state = state();
    state.update(Event::TabUpdate(
        (0..10).map(|i| tab(i + 20, i, i == 5)).collect(),
    ));
    state.render(5, 32);
    assert_eq!(
        state.row_targets,
        [Some(4), Some(5), Some(6), Some(7), Some(8)]
    );
    state.update(Event::Mouse(Mouse::LeftClick(0, 0)));
    state.update(Event::Mouse(Mouse::LeftClick(4, 0)));
    state.update(Event::Mouse(Mouse::ScrollDown(1)));
    state.update(Event::Mouse(Mouse::ScrollUp(1)));
    state.update(Event::Mouse(Mouse::LeftClick(-1, 0)));
    assert_eq!(state.host.switches, [4, 8, 7, 5]);
}

#[test]
fn compiled_configuration_preserves_aliases_and_explicit_variable_widths() {
    let mut state = State::default();
    state.load(BTreeMap::from([
        ("format_active".into(), "{index}:{=5:title}".into()),
        ("border_char".into(), "|".into()),
        ("start_index".into(), "0".into()),
    ]));
    state.update(Event::PermissionRequestResult(PermissionStatus::Granted));
    state.update(Event::PaneUpdate(manifest(0)));
    state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
    state.render(2, 10);
    assert_eq!(
        state.host.frames.last().unwrap(),
        "0:pr...  |\x1b[m\n         |\x1b[m"
    );
}

#[test]
fn restored_own_location_suppresses_background_refreshes() {
    let mut state = state();
    state.update(Event::TabUpdate(vec![tab(10, 0, true), tab(20, 1, false)]));
    state.update(Event::PaneUpdate(manifest(1)));
    assert!(!state.own_tab_is_active());
    state.update(Event::PaneUpdate(PaneManifest::default()));
    assert!(state.own_tab_is_active());
    assert!(!state.update(Event::PaneUpdate(manifest(1))));
    assert!(!state.own_tab_is_active());
}

#[test]
fn terminal_with_the_same_numeric_id_is_not_the_plugins_location() {
    let mut state = state();
    let mut panes = manifest(2);
    panes.panes.insert(
        0,
        vec![PaneInfo {
            id: 4,
            is_plugin: false,
            ..PaneInfo::default()
        }],
    );
    state.update(Event::PaneUpdate(panes));
    assert_eq!(state.own_tab_position, Some(2));
}

#[test]
fn diagnostics_are_opt_in_and_do_not_change_frames() {
    let mut quiet = state();
    let mut verbose = State::default();
    verbose.load(BTreeMap::from([("diagnostics".into(), "true".into())]));
    verbose.update(Event::PermissionRequestResult(PermissionStatus::Granted));
    for state in [&mut quiet, &mut verbose] {
        state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
        state.render(3, 20);
    }
    assert!(quiet.host.logs.is_empty());
    assert_eq!(quiet.host.frames, verbose.host.frames);
    assert!(
        verbose
            .host
            .logs
            .iter()
            .any(|line| line.contains("event=render") && line.contains("size=Some((3, 20))"))
    );
}

#[test]
fn diagnostics_capture_recovery_and_permission_context_without_content() {
    let mut state = State::default();
    state.load(BTreeMap::from([("diagnostics".into(), "true".into())]));
    let mut sensitive = tab(10, 0, false);
    sensitive.name = "PRIVATE-title\nforged-log".into();
    state.update(Event::TabUpdate(vec![sensitive]));
    state.update(Event::PermissionRequestResult(PermissionStatus::Denied));
    state.update(Event::PermissionRequestResult(PermissionStatus::Granted));
    state.update(Event::PaneUpdate(PaneManifest::default()));
    state.pipe(message("activity", "PRIVATE-payload"));
    let logs = state.host.logs.join("\n");
    for event in [
        "loaded",
        "permission_wait",
        "permission_denied",
        "permission_granted",
        "active_marker_missing",
        "own_pane_missing",
        "invalid_activity",
    ] {
        assert!(
            logs.contains(&format!("event={event}")),
            "missing {event}: {logs}"
        );
    }
    assert!(logs.contains("plugin_id=Some(4)"));
    assert!(logs.contains("pending=1"));
    assert!(!logs.contains("PRIVATE"));
    assert!(!logs.contains("forged-log"));
}

#[test]
fn diagnostics_distinguish_skipped_renders_from_background_refreshes() {
    let mut state = State::default();
    state.load(BTreeMap::from([("diagnostics".into(), "true".into())]));
    state.render(5, 32);
    state.update(Event::PermissionRequestResult(PermissionStatus::Granted));
    state.update(Event::TabUpdate(vec![tab(10, 0, true), tab(20, 1, false)]));
    state.update(Event::PaneUpdate(manifest(1)));
    let logs = state.host.logs.join("\n");
    assert!(logs.contains("event=render_skipped"));
    assert!(logs.contains("event=refresh_suppressed"));
    assert!(logs.contains("active_tab_id=Some(10) own_tab_position=Some(1)"));
    assert!(state.host.frames.is_empty());
}

#[test]
fn repeated_diagnostics_are_sampled_at_powers_of_two() {
    let mut state = State::default();
    state.load(BTreeMap::from([("diagnostics".into(), "true".into())]));
    for _ in 0..1024 {
        state.pipe(message("activity", "invalid"));
    }
    let logs: Vec<_> = state
        .host
        .logs
        .iter()
        .filter(|line| line.contains("event=invalid_activity "))
        .collect();
    assert_eq!(logs.len(), 11);
    assert!(logs.last().unwrap().contains("count=1024 "));
}

fn message(name: &str, payload: &str) -> PipeMessage {
    PipeMessage::new(
        PipeSource::Keybind,
        name,
        &Some(payload.into()),
        &None,
        false,
    )
}

#[test]
fn activity_pipe_renders_matching_rows_and_empty_payload_clears_them() {
    let mut state = state();
    state.own_session = "test".into();
    state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
    assert!(!state.pipe(message("activity", "not json")));
    assert!(state.activity.is_empty());
    assert!(state.pipe(message(
        "activity",
        r#"{"zsession":"test","name":"work-10","todos":[{"text":"build"}]}"#
    )));
    state.render(3, 32);
    assert!(state.host.frames.last().unwrap().contains("build"));
    assert!(state.pipe(message(
        "activity",
        r#"{"zsession":"test","name":"work-10","todos":[],"subagents":{}}"#
    )));
    state.render(3, 32);
    assert!(!state.host.frames.last().unwrap().contains("build"));
}

#[test]
fn selectable_pipes_change_host_focus_policy_without_redrawing() {
    let mut state = state();
    assert!(!state.pipe(message("set_selectable", "true")));
    assert!(state.is_selectable);
    assert_eq!(state.host.calls.last(), Some(&"selectable"));
    assert!(!state.pipe(message("toggle_selectable", "")));
    assert!(!state.is_selectable);
    assert_eq!(state.host.calls.last(), Some(&"non-selectable"));
    let call_count = state.host.calls.len();
    assert!(!state.pipe(message("set_selectable", "invalid")));
    assert_eq!(state.host.calls.len(), call_count);
}
