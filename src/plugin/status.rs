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
        } => {
            if statuses
                .get(&pane_id)
                .is_some_and(|current| current.runtime_id == runtime_id && current.seq >= seq)
            {
                return Some(false);
            }
            let changed = statuses.get(&pane_id).is_none_or(|current| {
                current.runtime_id != runtime_id || current.mode != mode || current.seq != seq
            });
            statuses.insert(
                pane_id,
                AgentStatus {
                    runtime_id,
                    seq,
                    mode,
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
