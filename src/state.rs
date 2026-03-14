use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json;

use crate::types::{Phase, ProjectState, Roster, Vote};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub struct State {
    pub path: PathBuf,
    pub data: ProjectState,
}

impl State {
    /// Create or load state.json at `path`.
    pub fn new(path: PathBuf) -> io::Result<Self> {
        if path.exists() {
            let data = Self::load(&path)?;
            Ok(Self { path, data })
        } else {
            let data = ProjectState {
                phase: Phase::Brainstorming,
                current_step_id: None,
                artifacts: HashMap::new(),
                approvals: HashMap::new(),
                decision_log: Vec::new(),
            };
            let state = Self { path, data };
            state.save()?;
            Ok(state)
        }
    }

    /// Deserialize ProjectState from file.
    pub fn load(path: &Path) -> io::Result<ProjectState> {
        let contents = fs::read_to_string(path)?;
        serde_json::from_str(&contents)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Serialize ProjectState to file (atomic write via temp file + rename).
    pub fn save(&self) -> io::Result<()> {
        let json = serde_json::to_string_pretty(&self.data)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let tmp_path = parent.join(".state.tmp");

        fs::write(&tmp_path, &json)?;
        fs::rename(&tmp_path, &self.path)?;

        Ok(())
    }

    /// Update the approvals map for `agent`.
    pub fn record_vote(&mut self, agent: &str, vote: Vote) -> io::Result<()> {
        self.data.approvals.insert(agent.to_string(), vote);
        self.save()
    }

    /// Returns true if every agent in `roster` has voted Approved.
    pub fn check_consensus(&self, roster: &Roster) -> bool {
        roster.0.iter().all(|agent| {
            self.data
                .approvals
                .get(&agent.name)
                .map(|v| v == &Vote::Approved)
                .unwrap_or(false)
        })
    }

    /// Returns true if the artifact `name` is present.
    pub fn has_artifact(&self, name: &str) -> bool {
        self.data.artifacts.contains_key(name)
    }

    /// Insert or update an artifact entry.
    pub fn add_artifact(&mut self, name: &str, path: &str) -> io::Result<()> {
        self.data
            .artifacts
            .insert(name.to_string(), path.to_string());
        self.save()
    }

    /// Append a decision to the log.
    pub fn add_decision(&mut self, decision: &str) -> io::Result<()> {
        self.data.decision_log.push(decision.to_string());
        self.save()
    }

    /// Transition to `next` phase and reset all approvals to Pending.
    pub fn advance_phase(&mut self, next: Phase) -> io::Result<()> {
        self.data.phase = next;
        for vote in self.data.approvals.values_mut() {
            *vote = Vote::Pending;
        }
        self.save()
    }

    /// Returns true if any agent has voted Blocking.
    pub fn is_blocked(&self) -> bool {
        self.data
            .approvals
            .values()
            .any(|v| v == &Vote::Blocking)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AgentProfile;
    use tempfile::tempdir;

    fn make_roster(names: &[&str]) -> Roster {
        Roster(
            names
                .iter()
                .map(|&n| AgentProfile {
                    name: n.to_string(),
                    role: "dev".to_string(),
                    personality: "".to_string(),
                    capabilities: vec![],
                    trigger_keywords: vec![],
                    preferred_successors: vec![],
                    allowed_tools: vec![],
                })
                .collect(),
        )
    }

    #[test]
    fn creates_new_state_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let state = State::new(path.clone()).unwrap();
        assert!(path.exists());
        assert_eq!(state.data.phase, Phase::Brainstorming);
    }

    #[test]
    fn loads_existing_state() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        {
            let mut s = State::new(path.clone()).unwrap();
            s.add_decision("initial decision").unwrap();
        }
        let s2 = State::new(path).unwrap();
        assert_eq!(s2.data.decision_log, vec!["initial decision"]);
    }

    #[test]
    fn record_vote_and_consensus() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut state = State::new(path).unwrap();
        let roster = make_roster(&["alice", "bob"]);

        assert!(!state.check_consensus(&roster));

        state.record_vote("alice", Vote::Approved).unwrap();
        assert!(!state.check_consensus(&roster));

        state.record_vote("bob", Vote::Approved).unwrap();
        assert!(state.check_consensus(&roster));
    }

    #[test]
    fn consensus_fails_on_non_approved() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut state = State::new(path).unwrap();
        let roster = make_roster(&["alice", "bob"]);

        state.record_vote("alice", Vote::Approved).unwrap();
        state.record_vote("bob", Vote::Discussing).unwrap();
        assert!(!state.check_consensus(&roster));
    }

    #[test]
    fn artifacts() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut state = State::new(path).unwrap();

        assert!(!state.has_artifact("spec.md"));
        state.add_artifact("spec.md", "/tmp/spec.md").unwrap();
        assert!(state.has_artifact("spec.md"));
    }

    #[test]
    fn advance_phase_resets_approvals() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut state = State::new(path).unwrap();

        state.record_vote("alice", Vote::Approved).unwrap();
        state.record_vote("bob", Vote::Approved).unwrap();
        state.advance_phase(Phase::Designing).unwrap();

        assert_eq!(state.data.phase, Phase::Designing);
        for vote in state.data.approvals.values() {
            assert_eq!(vote, &Vote::Pending);
        }
    }

    #[test]
    fn is_blocked() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut state = State::new(path).unwrap();

        assert!(!state.is_blocked());
        state.record_vote("alice", Vote::Blocking).unwrap();
        assert!(state.is_blocked());
    }

    #[test]
    fn atomic_write_persists_across_reload() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut state = State::new(path.clone()).unwrap();

        state.add_decision("d1").unwrap();
        state.add_decision("d2").unwrap();
        state.add_artifact("out.bin", "/builds/out.bin").unwrap();

        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.decision_log.len(), 2);
        assert!(loaded.artifacts.contains_key("out.bin"));
    }
}
