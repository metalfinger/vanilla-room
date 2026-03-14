use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;

use chrono::Utc;

use crate::agent::AgentExecutor;
use crate::context;
use crate::discussion::Discussion;
use crate::git_workspace::GitWorkspace;
use crate::parser;
use crate::persona;
use crate::playbook as pb;
use crate::state::State;
use crate::types::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_CONSECUTIVE: u32 = 1;
const TRANSCRIPT_WINDOW: usize = 30;
const MAX_TURNS: u32 = 100;
const MAX_RETRIES: u32 = 3;

// ---------------------------------------------------------------------------
// TurnResult
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum TurnResult {
    Continue,
    Paused,
    Complete,
    Error(String),
}

// ---------------------------------------------------------------------------
// Conductor
// ---------------------------------------------------------------------------

pub struct Conductor {
    config: RoomConfig,
    roster: Roster,
    playbook: Playbook,
    state: State,
    discussion: Discussion,
    git: GitWorkspace,
    executor: AgentExecutor,
    queue: VecDeque<String>,
    pending_handoffs: Vec<String>,
    reflexion_active: bool,
    reflexion_pair: Option<(String, String)>,
    reflexion_rounds: u32,
    max_reflexion_rounds: u32,
    turn_count: u32,
    paused: bool,
    personas_dir: PathBuf,
    #[allow(dead_code)]
    playbooks_dir: PathBuf,
    last_speaker: Option<String>,
}

impl Conductor {
    /// Initialize a new Conductor with full room configuration.
    pub fn new(
        config: RoomConfig,
        roster: Roster,
        playbook: Playbook,
        personas_dir: PathBuf,
        playbooks_dir: PathBuf,
    ) -> io::Result<Self> {
        let vr_dir = config.project_dir.join(".vanilla-room");
        std::fs::create_dir_all(&vr_dir)?;

        let state_path = vr_dir.join("state.json");
        let discussion_path = vr_dir.join("discussion.jsonl");

        let state = State::new(state_path)?;
        let discussion = Discussion::new(discussion_path)?;
        let git = GitWorkspace::new(config.repo_dir.clone(), &config.session_id)?;
        let executor = AgentExecutor::new(&config);

        // Set initial step if not already set
        let mut state = state;
        if state.data.current_step_id.is_none() && !playbook.steps.is_empty() {
            state.data.current_step_id = Some(playbook.steps[0].id.clone());
            state.save()?;
        }

        let queue: VecDeque<String> = roster.0.iter().map(|a| a.name.clone()).collect();

        Ok(Self {
            config,
            roster,
            playbook,
            state,
            discussion,
            git,
            executor,
            queue,
            pending_handoffs: Vec::new(),
            reflexion_active: false,
            reflexion_pair: None,
            reflexion_rounds: 0,
            max_reflexion_rounds: 3,
            turn_count: 0,
            paused: false,
            personas_dir,
            playbooks_dir,
            last_speaker: None,
        })
    }

    // -----------------------------------------------------------------------
    // Turn selection
    // -----------------------------------------------------------------------

    /// Choose the next agent to speak.
    pub fn pick_next(&mut self) -> Option<String> {
        // 1. Reflexion mode: alternate between fixer and rejector
        if self.reflexion_active {
            if let Some((ref fixer, ref rejector)) = self.reflexion_pair {
                let pick = if self.reflexion_rounds % 2 == 0 {
                    fixer.clone()
                } else {
                    rejector.clone()
                };
                return Some(pick);
            }
        }

        // 2. Pending handoffs
        if !self.pending_handoffs.is_empty() {
            return Some(self.pending_handoffs.remove(0));
        }

        // 3. Playbook requires a specific role for current step
        if let Some(ref step_id) = self.state.data.current_step_id.clone() {
            if let Some(role) = pb::required_role(&self.playbook, step_id) {
                if role == "ALL" {
                    // Pick any agent that hasn't voted yet
                    let pick = self.roster.0.iter().find(|a| {
                        !self
                            .state
                            .data
                            .approvals
                            .get(&a.name)
                            .is_some_and(|v| v == &Vote::Approved)
                    });
                    if let Some(agent) = pick {
                        let name = agent.name.clone();
                        self.rotate_to_back(&name);
                        return Some(name);
                    }
                } else if role.to_lowercase() == "conductor" {
                    // Conductor step — return "Conductor" to trigger auto-advance
                    return Some("Conductor".to_string());
                } else {
                    // Find an agent with the matching name (or role as fallback)
                    let pick = self
                        .roster
                        .0
                        .iter()
                        .find(|a| a.name == role || a.role.to_lowercase().contains(&role.to_lowercase()))
                        .map(|a| a.name.clone());
                    if let Some(ref name) = pick {
                        self.rotate_to_back(name);
                        return pick;
                    }
                    // Required role not on team — auto-advance past this step
                    if let Some(ref step_id) = self.state.data.current_step_id.clone() {
                        let _ = self.advance_step(step_id);
                    }
                    // Re-enter pick_next with updated step
                    return self.queue.front().cloned();
                }
            }
        }

        // 4. Round-robin from queue, skip if same as last speaker
        let queue_len = self.queue.len();
        for _ in 0..queue_len {
            if let Some(name) = self.queue.pop_front() {
                if MAX_CONSECUTIVE > 0
                    && self.last_speaker.as_deref() == Some(&name)
                    && queue_len > 1
                {
                    // Push to back and try next
                    self.queue.push_back(name);
                    continue;
                }
                self.queue.push_back(name.clone());
                return Some(name);
            }
        }

        // Fallback: just take whoever is front
        self.queue.front().cloned()
    }

    fn rotate_to_back(&mut self, name: &str) {
        if let Some(pos) = self.queue.iter().position(|n| n == name) {
            self.queue.remove(pos);
            self.queue.push_back(name.to_string());
        }
    }

    // -----------------------------------------------------------------------
    // Turn processing
    // -----------------------------------------------------------------------

    /// Execute a single agent turn.
    pub fn process_turn(&mut self, agent_name: &str) -> io::Result<TurnResult> {
        self.turn_count += 1;

        // Handle "Conductor" step — auto-advance, don't run as AI agent
        if agent_name == "Conductor" || agent_name == "conductor" {
            self.post_conductor_message("Finalizing — merging work and completing.")?;
            if let Some(ref step_id) = self.state.data.current_step_id.clone() {
                self.advance_step(step_id)?;
            }
            return Ok(TurnResult::Paused); // pause for user approval of final merge
        }

        // 1. Find agent profile from roster
        let agent = match self.roster.0.iter().find(|a| a.name == agent_name) {
            Some(a) => a.clone(),
            None => {
                // Try loading persona from disk
                match persona::load_persona(&self.personas_dir, agent_name) {
                    Ok(a) => a,
                    Err(_e) => {
                        // Unknown agent — log warning but DON'T advance the step
                        eprintln!("[Conductor] Warning: agent '{}' not found. Skipping turn.", agent_name);
                        self.turn_count -= 1; // Don't count failed lookup as a turn
                        return Ok(TurnResult::Continue);
                    }
                }
            }
        };

        println!(
            "[Turn {}] {} ({})",
            self.turn_count, agent.name, agent.role
        );

        // 2. Read last N transcript entries
        let entries = self.discussion.read_last(TRANSCRIPT_WINDOW)?;
        let transcript = Discussion::format_for_prompt(&entries);

        // 3. Read brief
        let brief_path = self.config.project_dir.join(".vanilla-room/brief.md");
        let brief = std::fs::read_to_string(&brief_path).unwrap_or_else(|_| {
            "No brief provided.".to_string()
        });

        // 4. Get diff for review context
        let diff = self.git.get_diff(&agent.name);
        let diff_ref = if diff.is_empty() { None } else { Some(diff.as_str()) };

        // 5. Build step instruction
        let step_instruction = self.state.data.current_step_id.as_ref().and_then(|sid| {
            pb::current_step(&self.playbook, sid).map(|step| {
                let lead = if step.required_role == agent.name || step.required_role == "ALL" {
                    "You are the lead for this step. Do the work described above."
                } else {
                    "Contribute your perspective on this step."
                };
                format!(
                    "Current step: {} — {}\nYou are {} ({}). {}",
                    step.id, step.description, agent.name, agent.role, lead
                )
            })
        });

        // 6. Build prompts
        let (system_prompt, user_prompt) = context::build_full_prompt(
            &agent,
            &brief,
            &transcript,
            &self.state.data,
            &self.playbook,
            &self.state.data.decision_log,
            diff_ref,
            step_instruction.as_deref(),
        );

        // 7. Ensure agent branch exists and checkout
        self.git.ensure_agent_branch(&agent.name)?;
        self.git.checkout_agent_branch(&agent.name)?;

        // 8. Execute agent turn with retry
        let mut response = None;
        for attempt in 0..=MAX_RETRIES {
            match self.executor.execute_turn(&agent, &system_prompt, &user_prompt) {
                Ok(r) => { response = Some(r); break; }
                Err(e) if attempt < MAX_RETRIES => {
                    let wait_secs = 2u64.pow(attempt);
                    eprintln!("[Conductor] Agent '{}' failed (attempt {}): {}. Retrying in {}s...",
                        agent.name, attempt + 1, e, wait_secs);
                    std::thread::sleep(std::time::Duration::from_secs(wait_secs));
                }
                Err(e) => {
                    return Ok(TurnResult::Error(format!(
                        "Agent '{}' failed after {} retries: {}", agent.name, MAX_RETRIES, e
                    )));
                }
            }
        }
        let response = response.unwrap();

        // 7. Commit any changes on agent branch
        let commit_msg = format!("{}: turn {}", agent.name, self.turn_count);
        let _ = self.git.commit_agent_work(&agent.name, &commit_msg);

        // 8. Parse response
        let parsed = parser::parse_response(&response);

        // 9. Create discussion entry and append
        let entry = DiscussionEntry {
            timestamp: Utc::now(),
            agent_name: agent.name.clone(),
            role: agent.role.clone(),
            content: parsed.raw_content.clone(),
            status_vote: parsed.status.clone(),
            handoff_targets: parsed.handoff_targets.clone(),
            decisions: parsed.decisions.clone(),
            artifacts: parsed.artifacts.clone(),
        };
        self.discussion.append(&entry)?;

        // 10. Process parsed response
        // Record vote
        if let Some(ref vote) = parsed.status {
            self.state.record_vote(&agent.name, vote.clone())?;
        }

        // Process handoffs (filter out "Conductor" — it's not an AI agent)
        for target in &parsed.handoff_targets {
            if target.to_lowercase() != "conductor" {
                self.pending_handoffs.push(target.clone());
            }
        }

        // Record decisions
        for decision in &parsed.decisions {
            self.state.add_decision(decision)?;
        }

        // Record artifacts
        for artifact in &parsed.artifacts {
            self.state.add_artifact(artifact, &agent.name)?;
        }

        // Handle recruit requests
        for recruit_name in &parsed.recruit_requests {
            let _ = self.recruit_agent(recruit_name);
        }

        // Handle deboard requests
        for deboard_name in &parsed.deboard_requests {
            self.eject_agent(deboard_name);
        }

        self.last_speaker = Some(agent.name.clone());

        // Check for blocking votes
        if self.state.is_blocked() {
            let blockers: Vec<String> = self.state.data.approvals.iter()
                .filter(|(_, v)| **v == Vote::Blocking)
                .map(|(k, _)| k.clone())
                .collect();
            self.post_conductor_message(&format!(
                "Blocked by: {}. Resolve before advancing.", blockers.join(", ")
            ))?;
            return if self.paused { Ok(TurnResult::Paused) } else { Ok(TurnResult::Continue) };
        }

        // 11. Check for rejection -> start reflexion (only if not already in one)
        if parsed.status == Some(Vote::Rejected) && !self.reflexion_active {
            self.start_reflexion(&agent.name)?;
        }

        // Handle approval during reflexion — only the rejector can resolve it
        if self.reflexion_active && parsed.status == Some(Vote::Approved) {
            if let Some((_, ref rejector)) = self.reflexion_pair {
                if rejector == agent_name {
                    self.end_reflexion()?;
                }
            }
        }

        // Increment reflexion rounds if active
        if self.reflexion_active {
            self.reflexion_rounds += 1;
            if self.reflexion_rounds >= self.max_reflexion_rounds * 2 {
                self.post_conductor_message(
                    "Reflexion limit reached. Pausing for user intervention.",
                )?;
                self.paused = true;
                return Ok(TurnResult::Paused);
            }
        }

        // 12. Check consensus / advance step
        if let Some(result) = self.check_advancement()? {
            return Ok(result);
        }

        // 13. Check if playbook complete
        if self.state.data.current_step_id.is_none() {
            return Ok(TurnResult::Complete);
        }

        // 14. Continue or paused
        if self.paused {
            Ok(TurnResult::Paused)
        } else {
            Ok(TurnResult::Continue)
        }
    }

    /// Main orchestration loop.
    pub fn run(&mut self) -> io::Result<TurnResult> {
        self.post_conductor_message("Room started. Let's begin.")?;
        self.run_loop()
    }

    fn run_loop(&mut self) -> io::Result<TurnResult> {
        loop {
            if self.paused {
                return Ok(TurnResult::Paused);
            }
            if self.turn_count >= MAX_TURNS {
                self.post_conductor_message("Turn limit reached. Pausing for user review.")?;
                return Ok(TurnResult::Paused);
            }

            let agent = match self.pick_next() {
                Some(a) => a,
                None => return Ok(TurnResult::Error("No agents in queue".into())),
            };

            let result = self.process_turn(&agent)?;
            match result {
                TurnResult::Continue => continue,
                other => return Ok(other),
            }
        }
    }

    // -----------------------------------------------------------------------
    // User / conductor messages
    // -----------------------------------------------------------------------

    /// Inject a user message into the transcript.
    pub fn handle_user_message(&self, content: &str) -> io::Result<()> {
        let entry = DiscussionEntry {
            timestamp: Utc::now(),
            agent_name: "[User]".to_string(),
            role: "user".to_string(),
            content: content.to_string(),
            status_vote: None,
            handoff_targets: vec![],
            decisions: vec![],
            artifacts: vec![],
        };
        self.discussion.append(&entry)
    }

    /// Post a conductor message to the transcript.
    pub fn post_conductor_message(&self, content: &str) -> io::Result<()> {
        let entry = DiscussionEntry {
            timestamp: Utc::now(),
            agent_name: "[Conductor]".to_string(),
            role: "orchestrator".to_string(),
            content: content.to_string(),
            status_vote: None,
            handoff_targets: vec![],
            decisions: vec![],
            artifacts: vec![],
        };
        self.discussion.append(&entry)?;
        println!("[Conductor] {}", content);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Roster management
    // -----------------------------------------------------------------------

    /// Load a persona and add it to the roster and queue.
    pub fn recruit_agent(&mut self, name: &str) -> io::Result<()> {
        match persona::load_persona(&self.personas_dir, name) {
            Ok(profile) => {
                // Don't add duplicates
                if !self.roster.0.iter().any(|a| a.name == profile.name) {
                    println!("[Conductor] Recruiting agent: {}", profile.name);
                    self.queue.push_back(profile.name.clone());
                    self.roster.0.push(profile);
                }
                Ok(())
            }
            Err(e) => {
                eprintln!("[Conductor] Failed to recruit '{}': {}", name, e);
                Ok(())
            }
        }
    }

    /// Remove an agent from the roster and queue.
    pub fn eject_agent(&mut self, name: &str) {
        println!("[Conductor] Ejecting agent: {}", name);
        self.roster.0.retain(|a| a.name != name);
        self.queue.retain(|n| n != name);
        self.pending_handoffs.retain(|n| n != name);
    }

    /// Pause the room.
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume the room and continue the main loop.
    pub fn resume(&mut self) -> io::Result<TurnResult> {
        self.paused = false;
        self.post_conductor_message("Room resumed.")?;
        self.run_loop()
    }

    // -----------------------------------------------------------------------
    // Reflexion
    // -----------------------------------------------------------------------

    fn start_reflexion(&mut self, rejector: &str) -> io::Result<()> {
        // The fixer is the last speaker before the rejector (who produced work)
        let fixer = self
            .last_speaker
            .clone()
            .filter(|s| s != rejector)
            .or_else(|| {
                // Find the last agent who produced an artifact
                self.state
                    .data
                    .artifacts
                    .values()
                    .last()
                    .cloned()
            })
            .unwrap_or_else(|| {
                // Fallback: pick first agent in queue that isn't the rejector
                self.queue
                    .iter()
                    .find(|n| n.as_str() != rejector)
                    .cloned()
                    .unwrap_or_default()
            });

        if fixer.is_empty() {
            return Ok(());
        }

        println!(
            "[Conductor] Reflexion started: {} (fixer) <-> {} (rejector)",
            fixer, rejector
        );

        self.reflexion_active = true;
        self.reflexion_pair = Some((fixer.clone(), rejector.to_string()));
        self.reflexion_rounds = 0;

        self.post_conductor_message(&format!(
            "Reflexion loop: {} must address {}'s rejection. Locked until resolved.",
            fixer, rejector
        ))
    }

    fn end_reflexion(&mut self) -> io::Result<()> {
        println!("[Conductor] Reflexion resolved.");
        self.reflexion_active = false;
        self.reflexion_pair = None;
        self.reflexion_rounds = 0;

        self.post_conductor_message("Reflexion resolved. Resuming normal turn order.")
    }

    // -----------------------------------------------------------------------
    // Consensus & advancement
    // -----------------------------------------------------------------------

    fn check_advancement(&mut self) -> io::Result<Option<TurnResult>> {
        let step_id = match self.state.data.current_step_id.clone() {
            Some(id) => id,
            None => return Ok(Some(TurnResult::Complete)),
        };

        let step = match pb::current_step(&self.playbook, &step_id) {
            Some(s) => s.clone(),
            None => return Ok(None),
        };

        let gate = match step.gate.as_deref() {
            Some(g) => g,
            None => {
                // No gate — auto-advance when the required role has completed
                // their turn. The agent did the work (code is on their branch).
                let role = &step.required_role;
                let role_spoke = self.last_speaker.as_ref()
                    .map(|s| {
                        self.roster.0.iter().any(|a| &a.name == s && (a.name == *role || a.role.to_lowercase().contains(&role.to_lowercase())))
                    })
                    .unwrap_or(false);
                if role_spoke || role == "ALL" {
                    return self.advance_step(&step_id);
                }
                return Ok(None);
            }
        };

        match gate {
            "unanimous_approval" => {
                if self.state.check_consensus(&self.roster) {
                    return self.advance_step(&step_id);
                }
            }
            "approval_or_rejection" => {
                // Any approval advances; rejection is handled by reflexion
                let has_approval = self
                    .state
                    .data
                    .approvals
                    .values()
                    .any(|v| v == &Vote::Approved);
                if has_approval && !self.reflexion_active {
                    return self.advance_step(&step_id);
                }
            }
            "user_approval" => {
                // Pause and wait for user
                self.post_conductor_message(
                    "Step requires user approval. Pausing for review.",
                )?;
                self.paused = true;
                return Ok(Some(TurnResult::Paused));
            }
            _ => {} // Unknown gate, ignore
        }

        Ok(None)
    }

    fn phase_for_step(&self, step_id: &str) -> Phase {
        let id = step_id.to_lowercase();
        if id.contains("design") || id.contains("brainstorm") { Phase::Designing }
        else if id.contains("implement") || id.contains("fix") || id.contains("refactor") { Phase::Implementing }
        else if id.contains("test") { Phase::Testing }
        else if id.contains("review") { Phase::Reviewing }
        else if id.contains("final") || id.contains("merge") { Phase::Finalizing }
        else if id.contains("research") || id.contains("analyz") || id.contains("investigat") { Phase::Brainstorming }
        else { self.state.data.phase.clone() }
    }

    fn advance_step(&mut self, current_step_id: &str) -> io::Result<Option<TurnResult>> {
        // Merge the lead agent's branch if this step produced work
        if let Some(step) = pb::current_step(&self.playbook, current_step_id) {
            let role = &step.required_role;
            if role != "ALL" && role.to_lowercase() != "conductor" {
                if let Some(lead) = self.roster.0.iter().find(|a| {
                    a.name == *role || a.role.to_lowercase().contains(&role.to_lowercase())
                }) {
                    let name = lead.name.clone();
                    self.post_conductor_message(&format!("Merging {}'s work to session branch.", name))?;
                    if let Err(e) = self.git.merge_agent_to_session(&name) {
                        self.post_conductor_message(&format!("Merge conflict from {}: {}. Pausing.", name, e))?;
                        self.paused = true;
                        return Ok(Some(TurnResult::Paused));
                    }
                }
            }
        }

        let next = pb::next_step(&self.playbook, current_step_id);

        match next {
            Some(next_step) => {
                let next_id = next_step.id.clone();
                let next_desc = next_step.description.clone();

                self.state.data.current_step_id = Some(next_id.clone());
                self.state.data.phase = self.phase_for_step(&next_id);
                // Reset approvals for new step
                for vote in self.state.data.approvals.values_mut() {
                    *vote = Vote::Pending;
                }
                self.state.save()?;

                self.post_conductor_message(&format!(
                    "Step complete. Advancing to: {} - {}",
                    next_id, next_desc
                ))?;

                Ok(None) // Continue with next step
            }
            None => {
                // No more steps
                self.state.data.current_step_id = None;
                self.state.data.phase = Phase::Complete;
                self.state.save()?;

                self.post_conductor_message("All steps complete. Room finished.")?;
                Ok(Some(TurnResult::Complete))
            }
        }
    }
}
