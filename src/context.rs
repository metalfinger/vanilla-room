use crate::types::*;

pub fn build_system_prompt(
    agent: &AgentProfile,
    state: &ProjectState,
    playbook: &Playbook,
) -> String {
    let step_label = match &state.current_step_id {
        Some(id) => playbook
            .steps
            .iter()
            .find(|s| &s.id == id)
            .map(|s| format!("{} — {}", s.id, s.description))
            .unwrap_or_else(|| id.clone()),
        None => "None".to_string(),
    };

    let artifacts_list = if state.artifacts.is_empty() {
        "None".to_string()
    } else {
        state
            .artifacts
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };

    let approvals_str = if state.approvals.is_empty() {
        "None".to_string()
    } else {
        let mut parts: Vec<String> = state
            .approvals
            .iter()
            .map(|(name, vote)| {
                let symbol = match vote {
                    Vote::Approved => "✓",
                    Vote::Rejected | Vote::Blocking => "✗",
                    Vote::Discussing | Vote::Pending => "?",
                };
                format!("{}{}", name, symbol)
            })
            .collect();
        parts.sort();
        parts.join(" ")
    };

    let tools_str = if agent.allowed_tools.is_empty() {
        "None".to_string()
    } else {
        agent.allowed_tools.join(", ")
    };

    let branch = format!("vr/{}", agent.name.to_lowercase());

    format!(
        "You are {name}, a {role}.\n\
         {personality}\n\
         \n\
         RULES:\n\
         - Read the transcript and respond naturally\n\
         - Vote with [STATUS: X] when ready (APPROVED, REJECTED, DISCUSSING, BLOCKING)\n\
         - Use [HANDOFF: X] to request next speaker\n\
         - Use [DECISION: X] to log decisions\n\
         - Use [ARTIFACT: X] to declare produced artifacts\n\
         - You can REJECT work that doesn't meet your standards — the room will fix it\n\
         - Be direct. Don't be polite for politeness sake\n\
         \n\
         CURRENT STATE:\n\
         Phase: {phase} | Step: {step}\n\
         Artifacts: {artifacts}\n\
         Approvals: {approvals}\n\
         Your branch: {branch}\n\
         Your tools: {tools}",
        name = agent.name,
        role = agent.role,
        personality = agent.personality,
        phase = state.phase,
        step = step_label,
        artifacts = artifacts_list,
        approvals = approvals_str,
        branch = branch,
        tools = tools_str,
    )
}

pub fn build_user_prompt(
    brief: &str,
    transcript: &str,
    state: &ProjectState,
    decisions: &[String],
    diff: Option<&str>,
    step_instruction: Option<&str>,
) -> String {
    let decisions_str = if decisions.is_empty() {
        "None yet".to_string()
    } else {
        decisions
            .iter()
            .enumerate()
            .map(|(i, d)| format!("{}. {}", i + 1, d))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let artifacts_str = if state.artifacts.is_empty() {
        "None".to_string()
    } else {
        state
            .artifacts
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let approvals_str = if state.approvals.is_empty() {
        "None".to_string()
    } else {
        let mut parts: Vec<String> = state
            .approvals
            .iter()
            .map(|(name, vote)| {
                let symbol = match vote {
                    Vote::Approved => "✓",
                    Vote::Rejected | Vote::Blocking => "✗",
                    Vote::Discussing | Vote::Pending => "?",
                };
                format!("{}{}", name, symbol)
            })
            .collect();
        parts.sort();
        parts.join(" ")
    };

    let diff_section = match diff {
        Some(d) if !d.trim().is_empty() => format!("CODE CHANGES (git diff)\n```\n{}\n```", d),
        _ => String::new(),
    };

    let task_section = match step_instruction {
        Some(instr) => format!("\nYOUR TASK THIS TURN\n{}\n", instr),
        None => String::new(),
    };

    format!(
        "PROJECT BRIEF\n\
         {brief}\n\
         \n\
         TRANSCRIPT\n\
         {transcript}\n\
         \n\
         CURRENT ARTIFACTS\n\
         {artifacts}\n\
         \n\
         {diff_section}\n\
         CURRENT APPROVALS\n\
         {approvals}\n\
         \n\
         DECISIONS SO FAR\n\
         {decisions}\n\
         {task_section}\n\
         YOUR TURN. Respond naturally.",
        brief = brief,
        transcript = transcript,
        artifacts = artifacts_str,
        diff_section = diff_section,
        approvals = approvals_str,
        decisions = decisions_str,
        task_section = task_section,
    )
}

pub fn build_full_prompt(
    agent: &AgentProfile,
    brief: &str,
    transcript: &str,
    state: &ProjectState,
    playbook: &Playbook,
    decisions: &[String],
    diff: Option<&str>,
    step_instruction: Option<&str>,
) -> (String, String) {
    let system = build_system_prompt(agent, state, playbook);
    let user = build_user_prompt(brief, transcript, state, decisions, diff, step_instruction);
    (system, user)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_agent() -> AgentProfile {
        AgentProfile {
            name: "Alice".to_string(),
            role: "Developer".to_string(),
            personality: "Sharp and pragmatic.".to_string(),
            capabilities: vec!["coding".to_string()],
            trigger_keywords: vec![],
            preferred_successors: vec![],
            allowed_tools: vec!["bash".to_string(), "read".to_string()],
        }
    }

    fn make_state() -> ProjectState {
        let mut approvals = HashMap::new();
        approvals.insert("Alice".to_string(), Vote::Approved);
        approvals.insert("Bob".to_string(), Vote::Pending);

        let mut artifacts = HashMap::new();
        artifacts.insert("spec.md".to_string(), "path/to/spec.md".to_string());

        ProjectState {
            phase: Phase::Implementing,
            current_step_id: Some("step-1".to_string()),
            artifacts,
            approvals,
            decision_log: vec!["Use async runtime".to_string()],
        }
    }

    fn make_playbook() -> Playbook {
        Playbook {
            name: "default".to_string(),
            description: "Default playbook".to_string(),
            steps: vec![PlaybookStep {
                id: "step-1".to_string(),
                description: "Write initial implementation".to_string(),
                required_role: "Developer".to_string(),
                output_artifact: None,
                gate: None,
                next: None,
            }],
        }
    }

    #[test]
    fn system_prompt_contains_agent_name() {
        let agent = make_agent();
        let state = make_state();
        let playbook = make_playbook();
        let prompt = build_system_prompt(&agent, &state, &playbook);
        assert!(prompt.contains("You are Alice, a Developer."));
    }

    #[test]
    fn system_prompt_contains_branch() {
        let agent = make_agent();
        let state = make_state();
        let playbook = make_playbook();
        let prompt = build_system_prompt(&agent, &state, &playbook);
        assert!(prompt.contains("Your branch: vr/alice"));
    }

    #[test]
    fn system_prompt_contains_phase_and_step() {
        let agent = make_agent();
        let state = make_state();
        let playbook = make_playbook();
        let prompt = build_system_prompt(&agent, &state, &playbook);
        assert!(prompt.contains("Phase: Implementing"));
        assert!(prompt.contains("step-1"));
    }

    #[test]
    fn system_prompt_approval_symbols() {
        let agent = make_agent();
        let state = make_state();
        let playbook = make_playbook();
        let prompt = build_system_prompt(&agent, &state, &playbook);
        assert!(prompt.contains("Alice✓"));
        assert!(prompt.contains("Bob?"));
    }

    #[test]
    fn system_prompt_tools() {
        let agent = make_agent();
        let state = make_state();
        let playbook = make_playbook();
        let prompt = build_system_prompt(&agent, &state, &playbook);
        assert!(prompt.contains("bash"));
        assert!(prompt.contains("read"));
    }

    #[test]
    fn user_prompt_structure() {
        let state = make_state();
        let decisions = vec!["Use tokio".to_string(), "Use serde".to_string()];
        let prompt = build_user_prompt("Build a CLI tool", "Alice: hello", &state, &decisions, None, None);
        assert!(prompt.contains("PROJECT BRIEF"));
        assert!(prompt.contains("Build a CLI tool"));
        assert!(prompt.contains("TRANSCRIPT"));
        assert!(prompt.contains("Alice: hello"));
        assert!(prompt.contains("DECISIONS SO FAR"));
        assert!(prompt.contains("1. Use tokio"));
        assert!(prompt.contains("2. Use serde"));
        assert!(prompt.contains("YOUR TURN. Respond naturally."));
    }

    #[test]
    fn user_prompt_no_decisions() {
        let state = make_state();
        let prompt = build_user_prompt("Brief", "Transcript", &state, &[], None, None);
        assert!(prompt.contains("None yet"));
    }

    #[test]
    fn build_full_prompt_returns_tuple() {
        let agent = make_agent();
        let state = make_state();
        let playbook = make_playbook();
        let decisions = vec!["Decision 1".to_string()];
        let (sys, usr) = build_full_prompt(&agent, "Brief", "Transcript", &state, &playbook, &decisions, None, None);
        assert!(!sys.is_empty());
        assert!(!usr.is_empty());
        assert!(sys.contains("You are Alice"));
        assert!(usr.contains("PROJECT BRIEF"));
    }

    #[test]
    fn system_prompt_no_step() {
        let agent = make_agent();
        let mut state = make_state();
        state.current_step_id = None;
        let playbook = make_playbook();
        let prompt = build_system_prompt(&agent, &state, &playbook);
        assert!(prompt.contains("Step: None"));
    }

    #[test]
    fn rejection_vote_shows_x_symbol() {
        let agent = make_agent();
        let mut state = make_state();
        state.approvals.insert("Charlie".to_string(), Vote::Rejected);
        state.approvals.insert("Dave".to_string(), Vote::Blocking);
        let playbook = make_playbook();
        let prompt = build_system_prompt(&agent, &state, &playbook);
        assert!(prompt.contains("Charlie✗"));
        assert!(prompt.contains("Dave✗"));
    }

    #[test]
    fn user_prompt_includes_diff_and_instruction() {
        let state = make_state();
        let prompt = build_user_prompt("Brief", "Transcript", &state, &[], Some("diff --git a/foo.rs"), Some("Review the code changes for bugs"));
        assert!(prompt.contains("CODE CHANGES"));
        assert!(prompt.contains("diff --git a/foo.rs"));
        assert!(prompt.contains("YOUR TASK THIS TURN"));
        assert!(prompt.contains("Review the code changes for bugs"));
    }

    #[test]
    fn user_prompt_omits_empty_diff() {
        let state = make_state();
        let prompt = build_user_prompt("Brief", "Transcript", &state, &[], None, None);
        assert!(!prompt.contains("CODE CHANGES"));
        assert!(!prompt.contains("YOUR TASK THIS TURN"));
    }
}
