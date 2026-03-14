use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Phase
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Brainstorming,
    Designing,
    Implementing,
    Testing,
    Reviewing,
    Finalizing,
    Complete,
    Paused,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Phase::Brainstorming => "Brainstorming",
            Phase::Designing => "Designing",
            Phase::Implementing => "Implementing",
            Phase::Testing => "Testing",
            Phase::Reviewing => "Reviewing",
            Phase::Finalizing => "Finalizing",
            Phase::Complete => "Complete",
            Phase::Paused => "Paused",
        };
        write!(f, "{}", s)
    }
}

// ---------------------------------------------------------------------------
// Vote
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Vote {
    Approved,
    Rejected,
    Discussing,
    Blocking,
    Pending,
}

impl fmt::Display for Vote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Vote::Approved => "Approved",
            Vote::Rejected => "Rejected",
            Vote::Discussing => "Discussing",
            Vote::Blocking => "Blocking",
            Vote::Pending => "Pending",
        };
        write!(f, "{}", s)
    }
}

// ---------------------------------------------------------------------------
// AgentProfile
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub name: String,
    pub role: String,
    pub personality: String,
    pub capabilities: Vec<String>,
    pub trigger_keywords: Vec<String>,
    pub preferred_successors: Vec<String>,
    pub allowed_tools: Vec<String>,
}

// ---------------------------------------------------------------------------
// Roster
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roster(pub Vec<AgentProfile>);

// ---------------------------------------------------------------------------
// ProjectState
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectState {
    pub phase: Phase,
    pub current_step_id: Option<String>,
    pub artifacts: HashMap<String, String>,
    pub approvals: HashMap<String, Vote>,
    pub decision_log: Vec<String>,
}

// ---------------------------------------------------------------------------
// DiscussionEntry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionEntry {
    pub timestamp: DateTime<Utc>,
    pub agent_name: String,
    pub role: String,
    pub content: String,
    pub status_vote: Option<Vote>,
    pub handoff_targets: Vec<String>,
    pub decisions: Vec<String>,
    pub artifacts: Vec<String>,
}

// ---------------------------------------------------------------------------
// PlaybookStep
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookStep {
    pub id: String,
    pub description: String,
    pub required_role: String,
    pub output_artifact: Option<String>,
    pub gate: Option<String>,
    pub next: Option<String>,
}

// ---------------------------------------------------------------------------
// Playbook
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    pub name: String,
    pub description: String,
    pub steps: Vec<PlaybookStep>,
}

// ---------------------------------------------------------------------------
// ParsedResponse
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedResponse {
    pub raw_content: String,
    pub status: Option<Vote>,
    pub handoff_targets: Vec<String>,
    pub decisions: Vec<String>,
    pub artifacts: Vec<String>,
    pub recruit_requests: Vec<String>,
    pub deboard_requests: Vec<String>,
}

// ---------------------------------------------------------------------------
// RoomConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomConfig {
    pub project_dir: PathBuf,
    pub repo_dir: PathBuf,
    pub session_id: String,
}
