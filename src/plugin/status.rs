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
        } => {
            if statuses
                .get(&pane_id)
                .is_some_and(|current| current.runtime_id == runtime_id && current.seq >= seq)
            {
                return Some(false);
            }
            let watchers: String = "CIPRS"
                .chars()
                .filter(|letter| watchers.contains(*letter))
                .collect();
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
