# Vanilla Room — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a CLI tool that orchestrates multiple Claude Code agents in a collaborative "room," enabling autonomous software development with debate, consensus, and git-based workspace isolation.

**Architecture:** Conductor loop drives turn-based agent execution. Agents are `claude -p` calls with personas. Communication via shared JSONL transcript. Code isolation via git branches. Smart recruiting via task analysis.

**Tech Stack:** Rust, Claude Code CLI (`claude -p`), Git, TOML (playbooks/personas)

---

## File Structure

```
src/
├── main.rs              ← CLI entry point (init, run, say, approve, status, recruit, eject, pause, resume)
├── conductor.rs         ← Turn queue, phase machine, handoffs, reflexion, consensus
├── agent.rs             ← Execute claude -p with persona + transcript, parse response
├── recruiter.rs         ← Analyze task → pick team + playbook
├── discussion.rs        ← JSONL transcript read/append
├── state.rs             ← Phase machine, approvals, artifact tracking
├── playbook.rs          ← Load TOML playbooks, step progression
├── persona.rs           ← Load TOML personas, build system prompts
├── git_workspace.rs     ← Branch management: create, checkout, merge, cleanup
├── context.rs           ← Build full prompt for each agent turn (persona + transcript + state)
├── parser.rs            ← Parse agent response: [STATUS], [HANDOFF], [DECISION], [ARTIFACT], [RECRUIT], [DEBOARD]
└── types.rs             ← Shared types: AgentProfile, ProjectState, PlaybookStep, DiscussionEntry, etc.

playbooks/
├── feature.toml         ← design → consensus → implement → test → review → finalize
├── bugfix.toml          ← investigate → fix → test → review → finalize
├── refactor.toml        ← plan → implement → test → review → finalize
├── investigation.toml   ← research → analyze → report
└── documentation.toml   ← outline → write → review → finalize

personas/
├── architect.toml
├── developer.toml
├── reviewer.toml
├── tester.toml
├── security.toml
├── devops.toml
├── researcher.toml
├── profiler.toml
├── technical_writer.toml
└── database_expert.toml

Cargo.toml               ← dependencies: clap, serde, serde_json, toml, chrono, uuid
```

---

## Task 1: Types and Core Data Structures

**Files:** Create `src/types.rs`

Define all shared types used across the system:

- [ ] `AgentProfile` — name, role, personality, capabilities, trigger_keywords, preferred_successors, allowed_tools
- [ ] `Roster` — Vec<AgentProfile>
- [ ] `ProjectState` — phase (enum), current_step_id, artifacts (HashMap), approvals (HashMap<String, Vote>), decision_log (Vec)
- [ ] `Phase` enum — Brainstorming, Designing, Implementing, Testing, Reviewing, Finalizing, Complete, Paused
- [ ] `Vote` enum — Approved, Rejected, Discussing, Blocking, Pending
- [ ] `DiscussionEntry` — timestamp, agent_name, role, content, status_vote (Option), handoff_targets (Vec), decisions (Vec), artifacts (Vec)
- [ ] `PlaybookStep` — id, description, required_role, output_artifact (Option), gate (Option), next (Option)
- [ ] `Playbook` — name, description, steps (Vec<PlaybookStep>)
- [ ] `ParsedResponse` — raw_content, status (Option<Vote>), handoff_targets (Vec<String>), decisions (Vec<String>), artifacts (Vec<String>), recruit_requests (Vec<String>), deboard_requests (Vec<String>)
- [ ] `RoomConfig` — project_dir (PathBuf), repo_dir (PathBuf), session_id (String)
- [ ] Add serde Serialize/Deserialize to all types
- [ ] Commit: `feat: define core types`

---

## Task 2: Discussion Log (the shared transcript)

**Files:** Create `src/discussion.rs`

The bus for all agent communication. Append-only JSONL file.

- [ ] `Discussion::new(path: PathBuf)` — create or open discussion.jsonl
- [ ] `Discussion::append(entry: &DiscussionEntry)` — serialize to JSON, append line to file
- [ ] `Discussion::read_last(n: usize) -> Vec<DiscussionEntry>` — read last N entries
- [ ] `Discussion::read_all() -> Vec<DiscussionEntry>` — read full transcript
- [ ] `Discussion::format_for_prompt(entries: &[DiscussionEntry]) -> String` — format as readable transcript:
  ```
  [Turn 1] Conductor: Objective: Add JWT auth...
  [Turn 2] Architect: I propose... [STATUS: DISCUSSING]
  [Turn 3] Security: Concern about... [STATUS: DISCUSSING]
  ```
- [ ] Test: write 5 entries, read_last(3), verify order and content
- [ ] Commit: `feat: discussion log JSONL read/write`

---

## Task 3: State Machine

**Files:** Create `src/state.rs`

Manages phase progression, approvals, and artifacts.

- [ ] `State::new(path: PathBuf)` — create or load state.json
- [ ] `State::load() -> ProjectState` — deserialize from file
- [ ] `State::save(state: &ProjectState)` — serialize to file (atomic write)
- [ ] `State::record_vote(agent: &str, vote: Vote)` — update approvals map
- [ ] `State::check_consensus(roster: &Roster) -> bool` — all agents voted Approved?
- [ ] `State::has_artifact(name: &str) -> bool`
- [ ] `State::add_artifact(name: &str, path: &str)`
- [ ] `State::add_decision(decision: &str)`
- [ ] `State::advance_phase(next: Phase)` — transition + reset approvals to Pending
- [ ] `State::is_blocked() -> bool` — any agent voted Blocking?
- [ ] Commit: `feat: state machine with approvals and artifacts`

---

## Task 4: Response Parser

**Files:** Create `src/parser.rs`

Parse structured tags from agent natural language responses.

- [ ] `parse_response(raw: &str) -> ParsedResponse`
- [ ] Extract `[STATUS: APPROVED]` / `REJECTED` / `DISCUSSING` / `BLOCKING`
- [ ] Extract `[HANDOFF: Developer]` or `[HANDOFF: Reviewer, Tester]`
- [ ] Extract `[DECISION: Use JWT with 1h expiry]`
- [ ] Extract `[ARTIFACT: design.md]`
- [ ] Extract `[RECRUIT: database_expert]`
- [ ] Extract `[DEBOARD: researcher]`
- [ ] Tags are case-insensitive, may appear anywhere in text
- [ ] Multiple tags of same type allowed (multiple decisions, etc.)
- [ ] Test with sample agent responses containing various tag combinations
- [ ] Commit: `feat: response parser for agent communication tags`

---

## Task 5: Persona Loader

**Files:** Create `src/persona.rs`, create all persona TOML files

- [ ] Define persona TOML format:
  ```toml
  [agent]
  name = "Developer"
  role = "Senior Developer"
  personality = "..."
  capabilities = ["implementation", "testing"]
  trigger_keywords = ["implement", "code", "build"]
  preferred_successors = ["Tester", "Reviewer"]

  [tools]
  allowed = ["Edit", "Write", "Read", "Bash", "Glob", "Grep"]
  ```
- [ ] `load_persona(name: &str) -> AgentProfile` — load from personas/{name}.toml
- [ ] `list_available() -> Vec<String>` — list all persona TOML files
- [ ] Create persona files:
  - [ ] `personas/architect.toml` — pragmatic, high-level design, adapts to feedback, prefers simplicity
  - [ ] `personas/developer.toml` — production-ready code, pushes back on complexity, tests alongside code
  - [ ] `personas/reviewer.toml` — thorough, direct, rejects buggy/insecure code, doesn't rubber-stamp
  - [ ] `personas/tester.toml` — adversarial, edge cases, blocks on missing tests, tries to break things
  - [ ] `personas/security.toml` — auth, encryption, input validation, data exposure, OWASP
  - [ ] `personas/devops.toml` — deployment, CI/CD, infrastructure, monitoring
  - [ ] `personas/researcher.toml` — investigation, profiling, root cause analysis, documentation
  - [ ] `personas/profiler.toml` — performance, benchmarks, bottleneck identification
  - [ ] `personas/technical_writer.toml` — docs, API specs, clear explanations
  - [ ] `personas/database_expert.toml` — queries, schema design, migrations, N+1 detection
- [ ] Commit: `feat: persona loader and 10 agent personalities`

---

## Task 6: Playbook Loader

**Files:** Create `src/playbook.rs`, create playbook TOML files

- [ ] `load_playbook(name: &str) -> Playbook` — load from playbooks/{name}.toml
- [ ] `current_step(playbook: &Playbook, step_id: &str) -> &PlaybookStep`
- [ ] `next_step(playbook: &Playbook, step_id: &str) -> Option<&PlaybookStep>`
- [ ] `required_role(playbook: &Playbook, step_id: &str) -> Option<String>`
- [ ] Create playbook files:
  - [ ] `playbooks/feature.toml` — design → consensus → implement → test → review → finalize
  - [ ] `playbooks/bugfix.toml` — investigate → reproduce → fix → test → review → finalize
  - [ ] `playbooks/refactor.toml` — analyze → plan → implement → test → review → finalize
  - [ ] `playbooks/investigation.toml` — research → analyze → report → consensus
  - [ ] `playbooks/documentation.toml` — outline → write → review → finalize
- [ ] Commit: `feat: playbook loader with 5 workflow templates`

---

## Task 7: Git Workspace Manager

**Files:** Create `src/git_workspace.rs`

Manages per-agent branches for code isolation.

- [ ] `GitWorkspace::new(repo_dir: PathBuf, session_id: &str)`
- [ ] `create_session_branch()` — create `vr/session-{id}` from current HEAD
- [ ] `create_agent_branch(agent_name: &str)` — create `vr/{agent_name}` from session branch
- [ ] `checkout_agent_branch(agent_name: &str)` — checkout for agent to work on
- [ ] `commit_agent_work(agent_name: &str, message: &str)` — stage all + commit on agent's branch
- [ ] `get_diff(agent_name: &str) -> String` — diff between agent branch and session branch
- [ ] `merge_agent_to_session(agent_name: &str) -> Result` — merge agent's work to session branch
- [ ] `merge_session_to_main() -> Result` — final merge (user-triggered)
- [ ] `cleanup_branches(session_id: &str)` — delete all vr/* branches for this session
- [ ] `restore_original_branch()` — checkout whatever branch user was on before
- [ ] All git operations via `std::process::Command` calling `git`
- [ ] Commit: `feat: git workspace with per-agent branch isolation`

---

## Task 8: Context Builder

**Files:** Create `src/context.rs`

Builds the full prompt for each agent turn.

- [ ] `build_system_prompt(agent: &AgentProfile, state: &ProjectState, playbook: &Playbook) -> String`
  - Agent identity + personality
  - Communication rules (STATUS, HANDOFF, DECISION tags)
  - Current phase and step
  - Available tools
  - Branch info
- [ ] `build_user_prompt(brief: &str, transcript: &str, state: &ProjectState, decisions: &[String]) -> String`
  - Project brief
  - Formatted transcript (last 30 messages)
  - Current artifacts list
  - Current approvals status
  - Decisions made so far
  - "Your turn" instruction
- [ ] `build_full_prompt(agent, brief, transcript, state, playbook, decisions) -> (String, String)`
  - Returns (system_prompt, user_prompt) tuple
- [ ] Commit: `feat: context builder for agent prompts`

---

## Task 9: Agent Executor

**Files:** Create `src/agent.rs`

Runs a single agent turn via `claude -p`.

- [ ] `AgentExecutor::new(config: &RoomConfig)`
- [ ] `execute_turn(agent: &AgentProfile, system_prompt: &str, user_prompt: &str) -> Result<String>`
  - Checkout agent's branch (if agent has write tools)
  - Build claude command: `claude -p "{user_prompt}" --system-prompt "{system}" --allowedTools {tools} --print --output-format text`
  - Run command, capture stdout
  - If agent has write tools, commit any file changes
  - Return response text
- [ ] `execute_thinking_turn(agent: &AgentProfile, system_prompt: &str, user_prompt: &str) -> Result<String>`
  - Same but with `--allowedTools Read,Glob,Grep` only (for review/discussion turns)
- [ ] Handle timeout (max 5 minutes per turn)
- [ ] Handle claude CLI errors (not installed, auth failure, etc.)
- [ ] Commit: `feat: agent executor via claude -p`

---

## Task 10: Smart Recruiter

**Files:** Create `src/recruiter.rs`

Analyzes the task and picks the right team + playbook.

- [ ] `recruit(task: &str) -> RecruitmentPlan`
  - `RecruitmentPlan { agents: Vec<String>, playbook: String, reasoning: String }`
- [ ] Implementation: run `claude -p` with a meta-prompt:
  ```
  Given this task: "{task}"

  Available agents: {list from personas/}
  Available playbooks: {list from playbooks/}

  Pick the optimal team and playbook. Consider:
  - What skills does the task require?
  - Don't over-staff — fewer agents = faster consensus
  - Always include a Reviewer for code tasks
  - Include Security for auth/data tasks

  Respond as JSON: {"agents": [...], "playbook": "...", "reasoning": "..."}
  ```
- [ ] Parse JSON response, validate agent names exist in personas/
- [ ] Fallback: if parse fails, use default team [Architect, Developer, Reviewer] + feature playbook
- [ ] Commit: `feat: smart recruiter for task-based team selection`

---

## Task 11: The Conductor

**Files:** Create `src/conductor.rs`

The heart of Vanilla Room. Manages the turn queue, phases, handoffs, reflexion, and consensus.

- [ ] `Conductor::new(config: RoomConfig, roster: Roster, playbook: Playbook)`
- [ ] Core state:
  ```rust
  queue: VecDeque<String>,           // agent names in turn order
  pending_handoffs: Vec<(String, Vec<String>)>,  // (from, targets)
  reflexion_active: bool,
  reflexion_pair: Option<(String, String)>,  // (fixer, rejector)
  consecutive_turns: HashMap<String, u32>,
  max_reflexion_rounds: u32,         // default 3
  turn_count: u32,
  ```
- [ ] `pick_next() -> Option<String>` — choose next agent:
  1. If reflexion active: return reflexion target or rejector
  2. If pending handoffs: promote handoff target to front
  3. If playbook requires a role: prioritize that role
  4. Otherwise: next in queue (skip if same as last, MAX_CONSECUTIVE=1)
- [ ] `process_turn(agent_name: &str) -> TurnResult`
  1. Load persona, build context
  2. Execute agent turn
  3. Parse response
  4. Append to transcript
  5. Process votes, handoffs, decisions, artifacts
  6. Check for rejection → start reflexion
  7. Check for consensus → advance phase
  8. Check for recruit/deboard requests → modify roster
  9. Return TurnResult (Continue, Paused, Complete, Error)
- [ ] `run() -> Result<()>` — main loop:
  ```rust
  loop {
      let agent = self.pick_next()?;
      let result = self.process_turn(&agent)?;
      match result {
          TurnResult::Continue => continue,
          TurnResult::Paused => break,     // user needs to approve/input
          TurnResult::Complete => break,    // all done
          TurnResult::Error(e) => break,   // something went wrong
      }
  }
  ```
- [ ] `handle_user_message(content: &str)` — inject user message into transcript
- [ ] `recruit_agent(name: &str)` — load persona, add to roster + queue
- [ ] `eject_agent(name: &str)` — remove from roster + queue
- [ ] `pause()` / `resume()`
- [ ] Commit: `feat: conductor with turn queue, handoffs, reflexion, consensus`

---

## Task 12: CLI Entry Point

**Files:** Create `src/main.rs` with clap

- [ ] `vanilla-room init <task> [--repo <path>] [--playbook <name>]`
  - Create `.vanilla-room/` in repo root
  - Save brief.md
  - Run recruiter to pick team + playbook
  - Create session branch
  - Print: team, playbook, ready to run
- [ ] `vanilla-room run`
  - Load state, roster, playbook, discussion
  - Create Conductor
  - Run conductor loop
  - Print progress as it goes
- [ ] `vanilla-room status`
  - Print: phase, step, queue, approvals, turn count
- [ ] `vanilla-room say <message>`
  - Append user message to transcript
  - If conductor is paused, print "Message added. Run `vanilla-room resume` to continue."
- [ ] `vanilla-room approve`
  - Record user approval, advance phase if at gate
- [ ] `vanilla-room reject <reason>`
  - Record user rejection, trigger reflexion
- [ ] `vanilla-room recruit <agent_name>`
  - Add agent to roster and queue
- [ ] `vanilla-room eject <agent_name>`
  - Remove agent from roster and queue
- [ ] `vanilla-room pause`
  - Signal conductor to pause after current turn
- [ ] `vanilla-room resume`
  - Resume conductor loop
- [ ] `vanilla-room transcript [--last <n>]`
  - Print formatted transcript
- [ ] `vanilla-room cleanup`
  - Delete .vanilla-room/ and vr/* branches
- [ ] Commit: `feat: CLI with init, run, say, approve, reject, recruit, eject, status, transcript, cleanup`

---

## Task 13: Cargo.toml Dependencies

**Files:** Modify `Cargo.toml`

- [ ] Add dependencies:
  ```toml
  [dependencies]
  clap = { version = "4", features = ["derive"] }
  serde = { version = "1", features = ["derive"] }
  serde_json = "1"
  toml = "0.8"
  chrono = { version = "0.4", features = ["serde"] }
  uuid = { version = "1", features = ["v4"] }
  ```
- [ ] Commit: `build: add dependencies`

---

## Task 14: Integration Test — Full Room Run

- [ ] Create a test repo with a simple Rust project
- [ ] Run `vanilla-room init "add a greet function that takes a name and returns Hello, {name}"`
- [ ] Verify: recruiter picks appropriate team
- [ ] Run `vanilla-room run`
- [ ] Verify: agents discuss, design, implement, test, review
- [ ] Verify: code is on a vr/ branch, not main
- [ ] Run `vanilla-room approve`
- [ ] Verify: merged to main, branches cleaned up
- [ ] Run `vanilla-room cleanup`
- [ ] Commit any fixes
- [ ] Push to GitHub

---

## Build Order

Tasks have dependencies:

```
Task 1 (types) ─────────────┬──────────────────────┐
                             │                      │
Task 13 (Cargo.toml) ───────┤                      │
                             │                      │
Task 2 (discussion) ─────┐  │                      │
Task 3 (state) ──────────┤  │                      │
Task 4 (parser) ─────────┤  │                      │
Task 5 (personas) ───────┤  │                      │
Task 6 (playbooks) ──────┤  │                      │
Task 7 (git workspace) ──┤  │                      │
                         │  │                      │
Task 8 (context) ────────┤ (needs 2,3,5,6)         │
Task 9 (agent) ──────────┤ (needs 7,8)             │
Task 10 (recruiter) ─────┤ (needs 5,6,9)           │
                         │                         │
Task 11 (conductor) ─────┤ (needs all above)       │
                         │                         │
Task 12 (CLI) ───────────┤ (needs 11)              │
                         │                         │
Task 14 (integration) ───┘                         │
```

**Recommended execution: Tasks 1+13 first, then 2-7 in parallel, then 8-10, then 11, then 12, then 14.**

---

## Estimated Effort

| Task | Lines (est) | Time |
|------|-------------|------|
| 1. Types | ~120 | 5 min |
| 2. Discussion | ~80 | 5 min |
| 3. State | ~100 | 5 min |
| 4. Parser | ~120 | 10 min |
| 5. Personas | ~30 each × 10 | 15 min |
| 6. Playbooks | ~30 each × 5 | 10 min |
| 7. Git Workspace | ~150 | 10 min |
| 8. Context Builder | ~100 | 10 min |
| 9. Agent Executor | ~120 | 10 min |
| 10. Recruiter | ~100 | 10 min |
| 11. Conductor | ~350 | 20 min |
| 12. CLI | ~150 | 10 min |
| 13. Cargo.toml | ~10 | 1 min |
| 14. Integration | ~60 min | testing |
| **Total** | **~1,700 lines** | **~2-3 hours** |
