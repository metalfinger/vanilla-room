use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::persona;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecruitmentPlan {
    pub agents: Vec<String>,
    pub playbook: String,
    pub reasoning: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn default_plan() -> RecruitmentPlan {
    RecruitmentPlan {
        agents: vec![
            "Architect".to_owned(),
            "Developer".to_owned(),
            "Reviewer".to_owned(),
        ],
        playbook: "feature".to_owned(),
        reasoning: "Fallback default plan.".to_owned(),
    }
}

/// List playbook names available in `playbooks_dir` (filenames without `.toml`).
fn list_playbooks(playbooks_dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(playbooks_dir) else {
        return vec![];
    };
    let mut names: Vec<String> = rd
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()?.to_str()? == "toml" {
                Some(path.file_stem()?.to_str()?.to_owned())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

/// Strip markdown code fences from a string if present.
fn strip_fences(s: &str) -> &str {
    let s = s.trim();
    // Handle ```json ... ``` or ``` ... ```
    let s = if s.starts_with("```") {
        // Drop the opening fence line
        let after_first = s.splitn(2, '\n').nth(1).unwrap_or(s);
        // Drop the closing fence if present
        if let Some(idx) = after_first.rfind("```") {
            after_first[..idx].trim()
        } else {
            after_first.trim()
        }
    } else {
        s
    };
    s
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Analyze `task` and return the optimal `RecruitmentPlan`.
///
/// Runs `claude -p` with a meta-prompt. On any failure (claude not available,
/// bad JSON, unknown agent/playbook names) falls back to the default plan.
pub fn recruit(task: &str, personas_dir: &Path, playbooks_dir: &Path) -> RecruitmentPlan {
    let available_agents = persona::list_available(personas_dir).unwrap_or_default();
    let available_playbooks = list_playbooks(playbooks_dir);

    let agents_list = available_agents.join(", ");
    let playbooks_list = available_playbooks.join(", ");

    let prompt = format!(
        r#"Given this task: "{task}"

Available agents: {agents_list}
Available playbooks: {playbooks_list}

Pick the optimal team and playbook. Consider:
- What skills does the task require?
- Don't over-staff — fewer agents = faster consensus
- Always include a Reviewer for code tasks
- Include Security for auth/data tasks

Respond ONLY with JSON, no markdown fences: {{"agents": [...], "playbook": "...", "reasoning": "..."}}"#
    );

    let child = Command::new("claude")
        .args(["--print", "--output-format", "text", "-p", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(_) => return default_plan(),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
    }

    let raw = match child.wait_with_output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => return default_plan(),
    };

    let json_str = strip_fences(&raw);

    let plan: RecruitmentPlan = match serde_json::from_str(json_str) {
        Ok(p) => p,
        Err(_) => return default_plan(),
    };

    // Validate agent names
    if !available_agents.is_empty() {
        let agents_lower: Vec<String> = available_agents
            .iter()
            .map(|a| a.to_lowercase())
            .collect();
        for agent in &plan.agents {
            if !agents_lower.contains(&agent.to_lowercase()) {
                return default_plan();
            }
        }
    }

    // Validate playbook name
    if !available_playbooks.is_empty()
        && !available_playbooks
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&plan.playbook))
    {
        return default_plan();
    }

    plan
}
