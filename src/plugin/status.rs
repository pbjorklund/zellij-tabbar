use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum AgentMode {
    Base,
    Working,
    Compacting,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentStatus {
    pub(super) runtime_id: String,
    pub(super) seq: u64,
    pub(super) mode: AgentMode,
    pub(super) watchers: String,
    pub(super) folder: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum StatusMessage {
    Snapshot {
        v: u8,
        runtime_id: String,
        seq: u64,
        pane_id: u32,
        mode: AgentMode,
        #[serde(default)]
        watchers: String,
        #[serde(default)]
        watcher_states: serde_json::Value,
        #[serde(default)]
        folder: serde_json::Value,
    },
    Remove {
        v: u8,
        runtime_id: String,
        seq: u64,
        pane_id: u32,
    },
}

pub(super) fn apply_status(
    statuses: &mut BTreeMap<u32, AgentStatus>,
    payload: &str,
) -> Option<bool> {
    let message: StatusMessage = serde_json::from_str(payload).ok()?;
    match message {
        StatusMessage::Snapshot {
            v: 1,
            runtime_id,
            seq,
            pane_id,
            mode,
            watchers,
            watcher_states,
            folder,
        } => {
            if statuses
                .get(&pane_id)
                .is_some_and(|current| current.runtime_id == runtime_id && current.seq >= seq)
            {
                return Some(false);
            }
            let watchers = detailed_watchers(&watcher_states).unwrap_or_else(|| {
                "CIPRS"
                    .chars()
                    .filter(|letter| watchers.contains(*letter))
                    .collect()
            });
            let folder = folder
                .as_str()
                .filter(|value| {
                    !value.is_empty()
                        && !value
                            .chars()
                            .any(|c| c.is_control() || c == '/' || c == '\\')
                })
                .map(str::to_owned);
            let changed = statuses.get(&pane_id).is_none_or(|current| {
                current.runtime_id != runtime_id || current.mode != mode || current.seq != seq
            });
            statuses.insert(
                pane_id,
                AgentStatus {
                    runtime_id,
                    seq,
                    mode,
                    watchers,
                    folder,
                },
            );
            Some(changed)
        }
        StatusMessage::Remove {
            v: 1,
            runtime_id,
            seq,
            pane_id,
        } => {
            let should_remove = statuses
                .get(&pane_id)
                .is_some_and(|current| current.runtime_id == runtime_id && seq > current.seq);
            if should_remove {
                statuses.remove(&pane_id);
            }
            Some(should_remove)
        }
        _ => None,
    }
}

fn detailed_watchers(value: &serde_json::Value) -> Option<String> {
    let entries = value.as_object()?;
    if entries.is_empty() {
        return Some(String::new());
    }
    let mut valid = false;
    let mut pairs = Vec::new();
    for letter in ['C', 'P', 'I', 'R', 'S'] {
        let key = letter.to_string();
        let Some(entry) = entries.get(&key) else {
            continue;
        };
        let code = match entry.get("status").and_then(serde_json::Value::as_str) {
            Some("off") => {
                valid = true;
                continue;
            }
            Some("polling") => 'p',
            Some("working") => 'w',
            Some("error") => 'e',
            Some("queued") => 'q',
            Some("paused") => 'a',
            Some("waiting") => {
                if entry
                    .get("waiting_kind")
                    .and_then(serde_json::Value::as_str)
                    == Some("human")
                {
                    'h'
                } else {
                    't'
                }
            }
            _ => continue,
        };
        valid = true;
        pairs.push(format!("{letter}{code}"));
    }
    valid.then(|| pairs.join("|"))
}

pub(super) fn marker(mode: AgentMode, frame: usize) -> &'static str {
    const WORKING: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    const COMPACTING: [&str; 4] = ["◐", "◓", "◑", "◒"];
    match mode {
        AgentMode::Base => "",
        AgentMode::Working => WORKING[frame % WORKING.len()],
        AgentMode::Compacting => COMPACTING[frame % COMPACTING.len()],
        AgentMode::Done => "●",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_snapshots_are_canonical_and_fenced_by_runtime_and_sequence() {
        let mut statuses = BTreeMap::new();
        let snapshot = |runtime: &str, seq: u64, watchers: &str| {
            format!(
                r#"{{"v":1,"kind":"snapshot","runtime_id":"{runtime}","seq":{seq},"pane_id":7,"mode":"working","watchers":"{watchers}"}}"#
            )
        };
        assert_eq!(
            apply_status(&mut statuses, &snapshot("one", 1, "SRR!pIC")),
            Some(true)
        );
        assert_eq!(statuses[&7].watchers, "CIRS");
        assert_eq!(
            apply_status(
                &mut statuses,
                r#"{"v":1,"kind":"snapshot","runtime_id":"one","seq":2,"pane_id":7,"mode":"working","watchers":"S\u001bR\nC"}"#
            ),
            Some(true)
        );
        assert_eq!(statuses[&7].watchers, "CRS");
        assert_eq!(
            apply_status(&mut statuses, &snapshot("one", 2, "P")),
            Some(false)
        );
        assert_eq!(statuses[&7].watchers, "CRS");
        assert_eq!(
            apply_status(&mut statuses, &snapshot("one", 0, "P")),
            Some(false)
        );
        assert_eq!(
            apply_status(&mut statuses, &snapshot("two", 1, "P")),
            Some(true)
        );
        assert_eq!(statuses[&7].watchers, "P");
        assert_eq!(
            apply_status(&mut statuses, &snapshot("two", 2, "")),
            Some(true)
        );
        assert!(statuses[&7].watchers.is_empty());
        assert_eq!(
            apply_status(&mut statuses, &snapshot("two", 3, "CIPRS")),
            Some(true)
        );
        assert_eq!(
            apply_status(
                &mut statuses,
                r#"{"v":1,"kind":"remove","runtime_id":"one","seq":9,"pane_id":7}"#
            ),
            Some(false)
        );
        assert_eq!(statuses[&7].watchers, "CIPRS");
        assert_eq!(
            apply_status(
                &mut statuses,
                r#"{"v":1,"kind":"remove","runtime_id":"two","seq":4,"pane_id":7}"#
            ),
            Some(true)
        );
        assert!(!statuses.contains_key(&7));
    }

    #[test]
    fn detailed_states_are_ordered_and_invalid_optional_data_preserves_activity() {
        let mut statuses = BTreeMap::new();
        let snapshot = |seq, detail: serde_json::Value, folder: serde_json::Value| {
            serde_json::json!({
                "v": 1, "kind": "snapshot", "runtime_id": "one", "seq": seq, "pane_id": 7,
                "mode": "working", "watchers": "CP", "watcher_states": detail, "folder": folder
            })
            .to_string()
        };
        for (i, (status, code)) in [
            ("polling", "p"),
            ("working", "w"),
            ("error", "e"),
            ("queued", "q"),
            ("paused", "a"),
            ("waiting", "t"),
        ]
        .iter()
        .enumerate()
        {
            let detail = serde_json::json!({"S":{"status":status},"R":{"status":status},"I":{"status":status},"P":{"status":status},"C":{"status":status}});
            apply_status(
                &mut statuses,
                &snapshot(i + 1, detail, serde_json::json!("界面")),
            );
            assert_eq!(
                statuses[&7].watchers,
                format!("C{code}|P{code}|I{code}|R{code}|S{code}")
            );
            assert_eq!(statuses[&7].folder.as_deref(), Some("界面"));
        }
        let cases = [
            (
                serde_json::json!({"C":{"status":"waiting","waiting_kind":"human"}}),
                "Ch",
            ),
            (
                serde_json::json!({"C":{"status":"waiting","waiting_kind":"bogus"}}),
                "Ct",
            ),
            (serde_json::json!({"C":{"status":"off"}}), ""),
            (serde_json::json!({}), ""),
            (
                serde_json::json!({"C":{"status":"bad"},"P":{"status":"working"},"Z":{"status":"error"}}),
                "Pw",
            ),
            (serde_json::json!({"C":{"status":"bad"}}), "CP"),
            (serde_json::json!(42), "CP"),
            (serde_json::Value::Null, "CP"),
        ];
        for (i, (detail, expected)) in cases.into_iter().enumerate() {
            apply_status(
                &mut statuses,
                &snapshot(i + 10, detail, serde_json::json!("/private/path")),
            );
            assert_eq!(statuses[&7].watchers, expected);
            assert_eq!(statuses[&7].folder, None);
            assert_eq!(statuses[&7].mode, AgentMode::Working);
        }
    }

    #[test]
    fn old_snapshots_and_invalid_watcher_types() {
        let mut statuses = BTreeMap::new();
        assert_eq!(
            apply_status(
                &mut statuses,
                r#"{"v":1,"kind":"snapshot","runtime_id":"one","seq":1,"pane_id":7,"mode":"base"}"#
            ),
            Some(true)
        );
        assert!(statuses[&7].watchers.is_empty());
        assert_eq!(
            apply_status(
                &mut statuses,
                r#"{"v":1,"kind":"snapshot","runtime_id":"one","seq":2,"pane_id":7,"mode":"base","watchers":42}"#
            ),
            None
        );
        assert_eq!(statuses[&7].seq, 1);
    }
}
