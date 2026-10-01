use super::formatting::parse_styled_string;
use super::*;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
struct RecordingHost {
    calls: Vec<&'static str>,
    switches: Vec<u32>,
    frames: Vec<String>,
    logs: Vec<String>,
    timeouts: Vec<f64>,
    cwd_results: BTreeMap<u32, std::path::PathBuf>,
    cwd_queries: Vec<u32>,
}

impl Host for RecordingHost {
    fn plugin_id(&mut self) -> u32 {
        4
    }
    fn get_pane_cwd(&mut self, pane_id: u32) -> Option<std::path::PathBuf> {
        self.cwd_queries.push(pane_id);
        self.cwd_results.get(&pane_id).cloned()
    }
    fn subscribe(&mut self, events: &[EventType]) {
        assert!(events.contains(&EventType::PermissionRequestResult));
        assert!(events.contains(&EventType::CwdChanged));
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
    fn set_timeout(&mut self, seconds: f64) {
        self.timeouts.push(seconds);
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
fn cwd_labels_refresh_without_changing_title_or_explicit_names() {
    let mut state = state();
    state.update(Event::TabUpdate(vec![TabInfo {
        name: "Tab #1".into(),
        ..tab(10, 0, true)
    }]));
    state.update(Event::PaneUpdate(manifest(0)));
    state.update(Event::CwdChanged(
        PaneId::Terminal(4),
        "/projects/worktree-task14".into(),
        vec![],
    ));
    state.render(1, 40);
    assert!(
        state
            .host
            .frames
            .last()
            .unwrap()
            .contains("1:worktree-task14 *")
    );

    state.update(Event::CwdChanged(
        PaneId::Terminal(4),
        "/projects/other".into(),
        vec![],
    ));
    state.render(1, 40);
    assert!(state.host.frames.last().unwrap().contains("1:other *"));
    state.style.apply(&BTreeMap::from([(
        "format_active".into(),
        "{name}|{title}".into(),
    )]));
    state.render(1, 40);
    assert!(state.host.frames.last().unwrap().contains("other|project"));
    state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
    state.render(1, 40);
    assert!(
        state
            .host
            .frames
            .last()
            .unwrap()
            .contains("work-10|project")
    );
}

fn default_tab(id: usize, position: usize, active: bool) -> TabInfo {
    TabInfo {
        name: format!("Tab #{}", position + 1),
        ..tab(id, position, active)
    }
}

#[test]
fn cwd_initial_lookup_waits_for_permission_and_is_cached_outside_render() {
    let mut state = State::default();
    state.host.cwd_results.insert(4, "/projects/initial".into());
    state.load(BTreeMap::new());
    state.update(Event::PaneUpdate(manifest(0)));
    state.update(Event::TabUpdate(vec![default_tab(90, 0, true)]));
    assert!(state.host.cwd_queries.is_empty());
    state.update(Event::PermissionRequestResult(PermissionStatus::Granted));
    state.render(1, 40);
    assert!(state.host.frames.last().unwrap().contains("1:initial *"));
    state.update(Event::PaneUpdate(manifest(0)));
    state.render(1, 40);
    assert_eq!(state.host.cwd_queries, [4]);
    assert!(!state.update(Event::CwdChanged(
        PaneId::Plugin(4),
        "/wrong".into(),
        vec![]
    )));
    assert!(state.update(Event::CwdChanged(
        PaneId::Terminal(4),
        "/projects/changed".into(),
        vec![]
    )));
    assert!(!state.update(Event::CwdChanged(
        PaneId::Terminal(4),
        "/projects/changed".into(),
        vec![]
    )));
    state.render(1, 40);
    assert!(state.host.frames.last().unwrap().contains("1:changed *"));
    assert_eq!(state.host.cwd_queries, [4]);
}

#[test]
fn cwd_focus_changes_do_not_change_status_selection_or_activity_matching() {
    let mut state = state();
    state.host.cwd_results = BTreeMap::from([
        (4, "/projects/first".into()),
        (9, "/worktrees/second".into()),
    ]);
    state.update(Event::TabUpdate(vec![default_tab(90, 0, true)]));
    let mut panes = manifest(0);
    panes.panes.get_mut(&0).unwrap().push(PaneInfo {
        id: 9,
        title: "second-title".into(),
        ..PaneInfo::default()
    });
    state.update(Event::PaneUpdate(panes.clone()));
    state.pipe(message("pi_status", r#"{"v":1,"kind":"snapshot","runtime_id":"first","seq":1,"pane_id":4,"mode":"working","watchers":"CIPRS"}"#));
    state.pipe(message(
        "activity",
        r#"{"name":"project","todos":[{"text":"first-todo"}]}"#,
    ));
    state.pipe(message(
        "activity",
        r#"{"name":"second-title","todos":[{"text":"second-todo"}]}"#,
    ));
    state.render(3, 40);
    assert!(state.host.frames.last().unwrap().contains("⠋ first"));
    assert!(state.host.frames.last().unwrap().contains("first-todo"));
    for pane in panes.panes.get_mut(&0).unwrap() {
        pane.is_focused = pane.id == 9 && !pane.is_plugin;
    }
    state.update(Event::PaneUpdate(panes));
    state.render(3, 40);
    let frame = state.host.frames.last().unwrap();
    assert!(frame.contains("⠋ second"));
    assert!(frame.contains("second-todo"));
    assert!(!frame.contains("first-todo"));
    click(&mut state, 1);
    assert_eq!(state.host.switches, [1]);
    assert_eq!(state.host.cwd_queries, [4, 9]);
}

#[test]
fn cwd_labels_follow_moves_and_closed_panes_are_pruned_in_both_orders() {
    for panes_first in [true, false] {
        let mut state = state();
        state.host.cwd_results.insert(4, "/worktrees/task14".into());
        state.update(Event::TabUpdate(vec![
            default_tab(90, 0, true),
            tab(20, 1, false),
        ]));
        state.update(Event::PaneUpdate(manifest(0)));
        let panes = Event::PaneUpdate(manifest(1));
        let tabs = Event::TabUpdate(vec![tab(20, 0, false), default_tab(90, 1, true)]);
        for event in if panes_first {
            [panes, tabs]
        } else {
            [tabs, panes]
        } {
            state.update(event);
        }
        state.render(3, 40);
        assert!(state.host.frames.last().unwrap().contains("2:task14 *"));
        click(&mut state, 1);
        assert_eq!(state.host.switches, [2]);
        assert_eq!(state.host.cwd_queries, [4]);
        let panes = Event::PaneUpdate(PaneManifest::default());
        let tabs = Event::TabUpdate(vec![default_tab(20, 0, true)]);
        for event in if panes_first {
            [panes, tabs]
        } else {
            [tabs, panes]
        } {
            state.update(event);
        }
        state.render(1, 40);
        assert!(!state.host.frames.last().unwrap().contains("task14"));
        assert!(state.pane_cwds.is_empty());
        state
            .host
            .cwd_results
            .insert(4, "/projects/reopened".into());
        state.update(Event::PaneUpdate(manifest(0)));
        state.render(1, 40);
        assert!(state.host.frames.last().unwrap().contains("1:reopened *"));
        assert_eq!(state.host.cwd_queries, [4, 4]);
    }
}

#[test]
fn cwd_missing_falls_back_without_title_path_inference_and_queries_only_once() {
    let mut state = state();
    state.update(Event::TabUpdate(vec![default_tab(90, 0, true)]));
    let mut panes = manifest(0);
    panes.panes.get_mut(&0).unwrap()[1].title = "bash /not/a/cwd".into();
    state.update(Event::PaneUpdate(panes.clone()));
    state.render(1, 40);
    assert!(
        state
            .host
            .frames
            .last()
            .unwrap()
            .contains("bash /not/a/cwd")
    );
    for title in ["", "Pane #4", "Tab #1"] {
        panes.panes.get_mut(&0).unwrap()[1].title = title.into();
        state.update(Event::PaneUpdate(panes.clone()));
        state.render(1, 40);
        assert!(state.host.frames.last().unwrap().contains("1:... *"));
    }
    assert_eq!(state.host.cwd_queries, [4]);
    state.update(Event::CwdChanged(
        PaneId::Terminal(4),
        "/projects/now-known".into(),
        vec![],
    ));
    state.render(1, 40);
    assert!(state.host.frames.last().unwrap().contains("1:now-known *"));
}

#[test]
fn cwd_paths_keep_unicode_width_and_sanitize_controls_with_a_root_fallback() {
    let mut state = state();
    state.update(Event::TabUpdate(vec![default_tab(90, 0, true)]));
    state.update(Event::PaneUpdate(manifest(0)));
    for (path, label) in [
        ("/", "/"),
        ("/projects/界e\u{301}/", "界e\u{301}"),
        ("/projects/a\nb\rc\td\u{1b}e", "a b c d e"),
    ] {
        state.update(Event::CwdChanged(PaneId::Terminal(4), path.into(), vec![]));
        state.render(1, 40);
        let frame = state.host.frames.last().unwrap().replace("\x1b[m", "");
        assert!(frame.contains(&format!("1:{label} *")), "{frame:?}");
        assert_eq!(frame.lines().count(), 1);
        assert!(!frame.contains(['\r', '\t', '\x1b']));
    }
    state.update(Event::CwdChanged(
        PaneId::Terminal(4),
        "/projects/界界界界界".into(),
        vec![],
    ));
    state.style.max_name_length = 5;
    state.render(1, 9);
    let frame = state.host.frames.last().unwrap().replace("\x1b[m", "");
    assert!(frame.contains("界..."));
    assert!(frame.width() <= 9);
    state.update(Event::TabUpdate(vec![TabInfo {
        name: String::new(),
        ..tab(90, 0, true)
    }]));
    state.mode_info.mode = InputMode::RenameTab;
    state.style.max_name_length = 20;
    state.render(1, 40);
    assert!(state.host.frames.last().unwrap().contains("Enter name..."));
}

#[test]
fn cwd_event_before_manifest_survives_permission_replay_without_querying() {
    let mut state = State::default();
    state.load(BTreeMap::new());
    state.update(Event::CwdChanged(
        PaneId::Terminal(4),
        "/early".into(),
        vec![],
    ));
    state.update(Event::PaneUpdate(manifest(0)));
    state.update(Event::TabUpdate(vec![default_tab(90, 0, true)]));
    state.update(Event::PermissionRequestResult(PermissionStatus::Granted));
    state.render(1, 40);
    assert!(state.host.frames.last().unwrap().contains("1:early *"));
    assert!(state.host.cwd_queries.is_empty());
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
        state.render(3, 30);
        click(&mut state, 1);
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
fn visible_sidebar_renders_when_another_client_owns_the_global_active_marker() {
    let mut state = state();
    state.update(Event::PaneUpdate(manifest(0)));
    assert!(!state.update(Event::TabUpdate(vec![tab(10, 0, false), tab(20, 1, true),])));

    assert!(state.update(Event::Visible(true)));
    let mut renamed = tab(10, 0, false);
    renamed.name = "visible to another client".into();
    assert!(state.update(Event::TabUpdate(vec![renamed, tab(20, 1, true)])));
    assert!(state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":1,"pane_id":4,"mode":"working"}"#,
    )));
    state.render(3, 30);

    let frame = state.host.frames.last().unwrap();
    assert!(frame.contains("⠋ visible to"));
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
    click(&mut state, 0);
    assert!(state.host.switches.is_empty());
    assert!(state.update(Event::TabUpdate(vec![tab(20, 0, true)])));
    state.render(2, 20);
    assert_eq!(
        state.row_actions,
        [
            Some(RowAction::SwitchTab {
                tab_id: 20,
                position: 0,
            }),
            None,
        ]
    );
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
    state.render(6, 30);
    assert_eq!(state.host.frames.last().unwrap().lines().count(), 6);
    assert_eq!(
        state.row_actions,
        [
            None,
            Some(RowAction::SwitchTab {
                tab_id: 10,
                position: 0,
            }),
            Some(RowAction::SwitchTab {
                tab_id: 10,
                position: 0,
            }),
            Some(RowAction::SwitchTab {
                tab_id: 10,
                position: 0,
            }),
            Some(RowAction::SwitchTab {
                tab_id: 20,
                position: 1,
            }),
            Some(RowAction::SwitchTab {
                tab_id: 30,
                position: 2,
            }),
        ]
    );
    click(&mut state, 2);
    assert_eq!(state.host.switches, [1]);
}

#[test]
fn activity_controls_preserve_physical_rows_and_click_targets() {
    for payload in [
        r#"{"name":"work-10","todos":[{"text":"first\nsecond\rthird\ttab"}]}"#,
        r#"{"name":"work-10","subagents":{"1":{"title":"first\nsecond\rthird\ttab"}}}"#,
    ] {
        let mut state = state();
        state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
        state.pipe(message("activity", payload));
        state.render(3, 60);
        let frame = state.host.frames.last().unwrap();
        assert_eq!(frame.lines().count(), 3, "{frame:?}");
        assert!(!frame.contains(['\r', '\t']));
        assert!(frame.contains("first second third tab"));
        assert_eq!(
            state.row_actions,
            [
                Some(RowAction::SwitchTab {
                    tab_id: 10,
                    position: 0,
                }),
                Some(RowAction::SwitchTab {
                    tab_id: 10,
                    position: 0,
                }),
                None,
            ]
        );
        click(&mut state, 1);
        click(&mut state, 2);
        assert_eq!(state.host.switches, [1]);
    }
}

#[test]
fn background_tabs_keep_status_and_activity_while_active_tabs_keep_activity() {
    let mut state = state();
    state.update(Event::TabUpdate(vec![tab(10, 0, true), tab(20, 1, false)]));
    state.pane_manifest.panes.insert(
        1,
        vec![PaneInfo {
            id: 9,
            ..PaneInfo::default()
        }],
    );
    state.statuses.insert(
        9,
        AgentStatus {
            runtime_id: "run".into(),
            seq: 1,
            mode: AgentMode::Done,
            watchers: "CIPRS".into(),
        },
    );
    state.pipe(message(
        "activity",
        r#"{"name":"work-10","todos":[{"text":"build"}]}"#,
    ));
    state.pipe(message(
        "activity",
        r#"{"name":"work-20","todos":[{"text":"waiting"}]}"#,
    ));

    state.render(8, 30);

    let frame = state.host.frames.last().unwrap();
    assert!(frame.contains("build"));
    assert!(frame.contains("CIRS● work-20"));
    assert!(frame.contains("waiting"));
    assert_eq!(
        state.row_actions[3],
        Some(RowAction::SwitchTab {
            tab_id: 20,
            position: 1
        })
    );
    assert!(state.row_actions[4..].iter().all(Option::is_none));
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
        state.row_actions,
        [
            Some(RowAction::SwitchTab {
                tab_id: 23,
                position: 3,
            }),
            Some(RowAction::SwitchTab {
                tab_id: 24,
                position: 4,
            }),
            Some(RowAction::SwitchTab {
                tab_id: 25,
                position: 5,
            }),
            Some(RowAction::SwitchTab {
                tab_id: 26,
                position: 6,
            }),
            Some(RowAction::SwitchTab {
                tab_id: 27,
                position: 7,
            }),
        ]
    );
    click(&mut state, 0);
    click(&mut state, 3);
    state.update(Event::Mouse(Mouse::ScrollDown(1)));
    state.update(Event::Mouse(Mouse::ScrollUp(1)));
    click(&mut state, -1);
    assert_eq!(state.host.switches, [4, 7, 7, 5]);
}

#[test]
fn click_release_switches_once_and_hold_does_not_activate_another_row() {
    let mut state = state();
    state.update(Event::TabUpdate(vec![tab(10, 0, true), tab(20, 1, false)]));
    state.render(6, 30);
    click(&mut state, 1);
    state.update(Event::Timer(0.15));
    assert_eq!(state.host.switches, [2]);
    state.update(Event::Mouse(Mouse::LeftClick(0, 0)));
    state.update(Event::Mouse(Mouse::Hold(1, 0)));
    state.update(Event::Timer(0.15));
    state.update(Event::Mouse(Mouse::Release(1, 0)));
    assert_eq!(state.host.switches, [2]);
}

#[test]
fn click_timer_switches_when_zellij_does_not_forward_mouse_release() {
    let mut state = state();
    state.update(Event::TabUpdate(vec![tab(10, 0, true), tab(20, 1, false)]));
    state.render(6, 30);

    state.update(Event::Mouse(Mouse::LeftClick(1, 0)));
    assert!(state.host.switches.is_empty());
    state.update(Event::Timer(0.15));

    assert_eq!(state.host.switches, [2]);
}

fn click(state: &mut State, row: isize) {
    state.update(Event::Mouse(Mouse::LeftClick(row, 0)));
    state.update(Event::Mouse(Mouse::Release(row, 0)));
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
fn pi_status_pipe_animates_in_the_visible_sidebar_without_renaming_tabs() {
    let mut state = state();
    state.update(Event::PaneUpdate(manifest(0)));
    state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
    state.update(Event::Visible(true));

    assert!(state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":1,"pane_id":4,"mode":"working"}"#,
    )));
    assert_eq!(state.host.timeouts, [0.5]);
    state.render(2, 30);
    assert!(state.host.frames.last().unwrap().contains("⠋ work-10"));

    assert!(state.update(Event::Timer(0.5)));
    state.render(2, 30);
    assert!(state.host.frames.last().unwrap().contains("⠙ work-10"));
    assert_eq!(state.host.timeouts, [0.5, 0.5]);
}

#[test]
fn watcher_prefix_survives_truncation_and_tracks_the_owning_pane() {
    let mut state = state();
    state.style.max_name_length = 8;
    state.update(Event::TabUpdate(vec![tab(10, 0, true), tab(20, 1, false)]));
    let mut panes = manifest(0);
    panes.panes.insert(
        1,
        vec![PaneInfo {
            id: 9,
            ..PaneInfo::default()
        }],
    );
    state.update(Event::PaneUpdate(panes));
    state.update(Event::Visible(true));
    assert!(state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":1,"pane_id":9,"mode":"working","watchers":"SRPICC"}"#,
    )));
    state.render(3, 30);
    let frame = state.host.frames.last().unwrap().replace("\x1b[m", "");
    assert!(frame.contains("2:CIRS⠋ .."), "{frame:?}");
    assert!(frame.contains("1:work-10 *"), "{frame:?}");
    assert_eq!(state.tabs[1].name, "work-20");
    for line in frame.lines() {
        assert!(line.width() <= 30);
    }

    assert!(state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":2,"pane_id":9,"mode":"done","watchers":"R"}"#,
    )));
    state.render(3, 30);
    let frame = state.host.frames.last().unwrap();
    assert!(frame.contains("2:R● wo..."), "{frame:?}");
    assert!(frame.contains('●'), "{frame:?}");
    assert!(state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":3,"pane_id":9,"mode":"base"}"#,
    )));
    state.render(3, 30);
    assert!(!state.host.frames.last().unwrap().contains("2:R"));
}

#[test]
fn watcher_and_marker_follow_first_status_pane_through_move_and_closure() {
    for pane_first in [true, false] {
        let mut state = state();
        state.update(Event::TabUpdate(vec![tab(10, 0, true), tab(90, 1, false)]));
        let mut panes = manifest(0);
        panes.panes.get_mut(&0).unwrap().push(PaneInfo {
            id: 9,
            ..PaneInfo::default()
        });
        state.update(Event::PaneUpdate(panes.clone()));
        state.update(Event::Visible(true));
        state.pipe(message("pi_status", r#"{"v":1,"kind":"snapshot","runtime_id":"first","seq":1,"pane_id":4,"mode":"compacting","watchers":"R"}"#));
        state.pipe(message("pi_status", r#"{"v":1,"kind":"snapshot","runtime_id":"second","seq":1,"pane_id":9,"mode":"working","watchers":"S"}"#));
        state.render(3, 30);
        let frame = state.host.frames.last().unwrap();
        assert!(frame.contains("1:R◐ work-10"), "{frame:?}");
        assert!(!frame.contains("S⠋"), "{frame:?}");
        let moved_panes = PaneManifest {
            panes: [(1, panes.panes.remove(&0).unwrap())].into(),
        };
        let moved_tabs = vec![tab(90, 0, false), tab(10, 1, true)];
        if pane_first {
            state.update(Event::PaneUpdate(moved_panes.clone()));
            state.update(Event::TabUpdate(moved_tabs));
        } else {
            state.update(Event::TabUpdate(moved_tabs));
            state.update(Event::PaneUpdate(moved_panes.clone()));
        }
        state.render(3, 30);
        let frame = state.host.frames.last().unwrap();
        assert!(frame.contains("2:R◐ work-10"), "{frame:?}");
        assert!(frame.contains("1:work-90"), "{frame:?}");
        click(&mut state, 1);
        assert_eq!(state.host.switches, [2]);
        let empty = PaneManifest::default();
        let remaining = vec![tab(90, 0, true)];
        if pane_first {
            state.update(Event::PaneUpdate(empty));
            state.update(Event::TabUpdate(remaining));
        } else {
            state.update(Event::TabUpdate(remaining));
            state.update(Event::PaneUpdate(empty));
        }
        state.render(3, 30);
        let frame = state.host.frames.last().unwrap();
        assert!(frame.contains("1:work-90 *"), "{frame:?}");
        assert!(!frame.contains("R◐") && !frame.contains("S⠋"), "{frame:?}");
    }
}

#[test]
fn viewed_done_keeps_watchers_and_sequence_fence_until_off_or_shutdown() {
    let mut state = state();
    state.update(Event::PaneUpdate(manifest(0)));
    state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
    let done = r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":7,"pane_id":4,"mode":"done","watchers":"SRPICC"}"#;
    assert!(!state.pipe(message("pi_status", done)));
    state.render(2, 30);
    assert!(state.host.frames.last().unwrap().contains("CIRS● work-10"));
    assert!(state.update(Event::Visible(true)));
    state.render(2, 30);
    assert!(state.host.frames.last().unwrap().contains("CIRS work-10"));
    let status = state.statuses.get(&4).unwrap();
    assert_eq!(status.mode, AgentMode::Base);
    assert_eq!(status.seq, 7);
    assert_eq!(status.runtime_id, "run-1");
    assert!(!state.pipe(message("pi_status", done)));
    assert!(!state.pipe(message("pi_status", r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":6,"pane_id":4,"mode":"working","watchers":"S"}"#)));
    state.render(2, 30);
    assert!(state.host.frames.last().unwrap().contains("CIRS work-10"));
    assert!(state.pipe(message("pi_status", r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":8,"pane_id":4,"mode":"base","watchers":""}"#)));
    state.render(2, 30);
    assert!(!state.host.frames.last().unwrap().contains("CIRS"));
    state.pipe(message("pi_status", r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":9,"pane_id":4,"mode":"base","watchers":"I"}"#));
    assert!(!state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"remove","runtime_id":"foreign","seq":99,"pane_id":4}"#
    )));
    assert!(state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"remove","runtime_id":"run-1","seq":10,"pane_id":4}"#
    )));
    state.render(2, 30);
    assert!(state.host.frames.last().unwrap().contains("1:work-10 *"));
    assert!(state.host.timeouts.is_empty());
}

#[test]
fn hidden_sidebar_retains_status_but_does_not_render_or_rearm_animation() {
    let mut state = state();
    state.update(Event::PaneUpdate(manifest(0)));
    state.update(Event::TabUpdate(vec![tab(10, 0, true)]));

    assert!(!state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":1,"pane_id":4,"mode":"working"}"#,
    )));
    assert!(state.host.timeouts.is_empty());
    assert!(!state.update(Event::Timer(0.5)));
    assert!(state.host.timeouts.is_empty());
}

#[test]
fn background_done_status_is_cleared_only_when_its_tab_becomes_visible() {
    let mut state = state();
    state.update(Event::PaneUpdate(manifest(0)));
    state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
    assert!(!state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"snapshot","runtime_id":"run-1","seq":1,"pane_id":4,"mode":"done"}"#,
    )));
    assert_eq!(
        state.statuses.get(&4).map(|status| status.mode),
        Some(AgentMode::Done)
    );

    assert!(state.update(Event::Visible(true)));
    assert!(!state.statuses.contains_key(&4));
}

#[test]
fn stale_status_updates_and_foreign_removals_are_ignored() {
    let mut state = state();
    state.update(Event::PaneUpdate(manifest(0)));
    state.update(Event::TabUpdate(vec![tab(10, 0, true)]));
    state.update(Event::Visible(true));
    state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"snapshot","runtime_id":"run-2","seq":2,"pane_id":4,"mode":"compacting"}"#,
    ));
    state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"snapshot","runtime_id":"run-2","seq":1,"pane_id":4,"mode":"working"}"#,
    ));
    state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"remove","runtime_id":"old-run","seq":9,"pane_id":4}"#,
    ));
    state.render(2, 30);
    assert!(state.host.frames.last().unwrap().contains("◐ work-10"));

    assert!(state.pipe(message(
        "pi_status",
        r#"{"v":1,"kind":"remove","runtime_id":"run-2","seq":3,"pane_id":4}"#,
    )));
    state.render(2, 30);
    assert!(!state.host.frames.last().unwrap().contains("◐ work-10"));
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
