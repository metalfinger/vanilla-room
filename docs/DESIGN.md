# Vanilla Room — Design Document

## Vision

Vanilla Room is an autonomous multi-agent orchestration system for software engineering. You give it a task, it recruits the right team, they debate and collaborate in a shared "room," and deliver production-ready code — all running on your Claude Code MAX subscription for free.

Think of it as a virtual dev team that sits in a room, passes the mic, debates architecture, writes code on branches, reviews each other's work, and only ships when everyone agrees.

## Core Metaphor: The Room

A **room** is a shared workspace where AI agents collaborate on a task. Unlike a pipeline (A→B→C→done), a room enables real conversation:

```
Pipeline:                           Room:
  Architect designs                   Architect: "Use microservices"
  Developer implements                Reviewer: "Overkill for this scope"
  Reviewer says "wrong"               Architect: "Fair point, monolith then"
  → work wasted                       Developer: "Agreed, starting impl"
                                      → consensus BEFORE expensive work
```

Agents don't just hand off — they **debate, reject, refine, and reach consensus**. The room has:

- **Brief** — the task/objective
- **Roster** — who's participating (dynamic, can change mid-task)
- **Transcript** — shared discussion log (every agent reads everything)
- **State** — current phase, artifacts, approvals
- **Artifacts** — code, plans, test results
- **Playbook** — the workflow guiding execution

## Architecture Overview

```
User: "add JWT auth to the REST API"
                │
         vanilla-room init
                │
         ┌──────┴──────┐
         │  Recruiter   │  ← analyzes task, picks team + playbook
         └──────┬──────┘
                │
         Team: Architect, Developer, Security, Reviewer, Tester
         Playbook: feature_development
                │
         vanilla-room run
                │
         ┌──────┴──────┐
         │  Conductor   │  ← manages turn queue, phases, consensus
         └──────┬──────┘
                │
    ┌───────────┼───────────┬───────────┐
    ▼           ▼           ▼           ▼
 Architect   Developer   Reviewer   Tester
    │           │           │          │
    └───────────┴───────────┴──────────┘
              Shared Transcript
              (discussion.jsonl)
                    +
              Git Branches
              (vr/developer, vr/tester, etc.)
```

## Agent Execution Model

Each "agent" is a `claude -p` (non-interactive Claude Code) call with:
- A deep personality (system prompt)
- The full transcript so far
- Current state and artifacts
- Tool access appropriate to their role

```bash
claude -p "{persona + transcript + instruction}" \
  --allowedTools Edit,Write,Read,Bash,Glob,Grep \
  --print
```

This is **free on MAX plan** — each agent turn uses the subscription, not API credits.

Each turn is **one-shot but contextual** — the agent doesn't retain memory between turns, but the transcript provides full continuity. This is how ReelMatrix works and it's proven effective.

## Git as Workspace Isolation

Each agent that writes code operates on its own git branch:

```
main (user's code, untouched)
  │
  ├── vr/session-abc123     (vanilla room's working branch)
  │     │
  │     ├── vr/architect    (design docs)
  │     ├── vr/developer    (implementation)
  │     ├── vr/tester       (test code)
  │     └── vr/security     (security fixes)
  │
  └── (user's other branches, untouched)
```

**Why git branches:**
- Each agent works in isolation — no conflicts
- Reviewer sees a **real git diff**, not pasted code
- Agent's work is **inspectable** at any point (`git log vr/developer`)
- If agent goes off-track, **reset the branch** — no damage
- Conductor merges when consensus is reached
- User gets a **clean PR** at the end

**Workflow:**
1. Agent checks out their branch: `git checkout -b vr/developer vr/session-abc123`
2. Agent writes code (claude -p with Edit/Write tools)
3. Agent commits: `git add -A && git commit -m "..."`
4. Reviewer checks out the branch and reads the diff
5. On approval, Conductor merges to `vr/session-abc123`
6. On final approval, user merges `vr/session-abc123` to `main`

## The Conductor

The conductor is the orchestrator. It is NOT an AI agent — it's deterministic Rust code that manages:

### Turn Queue
- Maintains ordered queue of agents
- Picks who speaks next based on:
  - Playbook requirements (which role is needed for current step)
  - Pending handoff requests
  - Reflexion loop state
  - Prevents same agent speaking twice consecutively (MAX_CONSECUTIVE = 1)

### Phase Machine
```
brainstorming → designing → implementing → testing → reviewing → finalizing → complete
```
Each phase has:
- Required artifacts to produce
- Required role to lead
- Gate condition to advance (unanimous approval or specific artifact)

### Handoffs
When an agent says `[HANDOFF: Developer]`, the Conductor prioritizes Developer next in the queue. This enables natural conversation flow.

### Reflexion Loops
When an agent says `[STATUS: REJECTED]`:
1. Conductor locks the queue to `[target_agent, rejecting_agent]`
2. Target fixes the work
3. Rejector reviews the fix
4. On approval, queue unlocks and normal flow resumes
5. This prevents the room from moving on with broken work

### Consensus Gates
At phase boundaries, ALL agents must vote `[STATUS: APPROVED]` before advancing. If any agent votes `[STATUS: BLOCKING]`, the phase stalls until the blocker is resolved.

## Smart Recruiting

The Recruiter analyzes the task and decides who to hire:

```
"Add JWT auth" → [Architect, Developer, Security, Reviewer, Tester]
                  playbook: feature_development

"Why is login slow?" → [Researcher, Developer, Profiler]
                       playbook: investigation

"Refactor auth module" → [Developer, Reviewer, Tester]
                         playbook: refactor

"Write API documentation" → [Technical Writer, Developer]
                            playbook: documentation
```

The recruiter is itself a `claude -p` call that receives the task and the available agent roster, then returns a JSON team selection.

### Dynamic Team Changes
At any point during the room:
- **Conductor can recruit** — if an agent is stuck on something outside the team's expertise, recruit a specialist
- **Conductor can deboard** — if an agent's role is complete, remove them from the queue
- **User can recruit** — `vanilla-room recruit database_expert`
- **User can eject** — `vanilla-room eject tester`

New agents join with full transcript context — they read everything that's happened so far.

## Agent Personas

Each agent has a deep personality defined in TOML:

```toml
[agent]
name = "Developer"
role = "Senior Developer"
personality = """You are a senior developer who writes production-ready
code. You push back on overengineered designs — simpler is better.
You ask clarifying questions before implementing. You write tests
alongside code. You prefer small, focused changes over big rewrites.
When reviewing designs, you think about: Can I actually build this?
What are the edge cases? Is there a simpler way?"""

capabilities = ["implementation", "testing", "debugging", "refactoring"]
trigger_keywords = ["implement", "code", "build", "fix", "develop"]
preferred_successors = ["Tester", "Reviewer"]

[tools]
allowed = ["Edit", "Write", "Read", "Bash", "Glob", "Grep"]
```

Personalities are NOT generic — they affect how agents collaborate:
- **Architect** is pragmatic, sketches high-level, adapts to feedback
- **Developer** pushes back on complexity, asks questions before coding
- **Reviewer** is direct, rejects work with bugs, doesn't rubber-stamp
- **Tester** is adversarial, tries to break things, blocks on missing tests
- **Security** focuses on auth, input validation, data exposure

## Playbooks

Playbooks define the workflow as a series of steps:

```toml
[playbook]
name = "feature_development"
description = "Build a new feature from scratch"

[[step]]
id = "design"
description = "Design the approach and architecture"
required_role = "Architect"
output_artifact = "design.md"
next = "consensus_design"

[[step]]
id = "consensus_design"
description = "All agents review and approve the design"
required_role = "ALL"
gate = "unanimous_approval"
next = "implement"

[[step]]
id = "implement"
description = "Write the code on a feature branch"
required_role = "Developer"
output_artifact = "code_changes"
next = "test"

[[step]]
id = "test"
description = "Write and run tests"
required_role = "Tester"
output_artifact = "test_results"
next = "review"

[[step]]
id = "review"
description = "Review code, tests, and security"
required_role = "Reviewer"
gate = "approval_or_rejection"
next = "finalize"

[[step]]
id = "finalize"
description = "Merge and present to user"
required_role = "Conductor"
output_artifact = "final_merge"
gate = "user_approval"
```

## User Interaction

The user is not passive. They can:

| Action | Command | What happens |
|--------|---------|--------------|
| **Watch** | HELIX shows room live | Read-only view of transcript |
| **Speak** | `vanilla-room say "use library X"` | Message appended to transcript, all agents see it |
| **Interrupt** | `vanilla-room pause` | Conductor pauses after current turn |
| **Resume** | `vanilla-room resume` | Conductor continues |
| **Approve** | `vanilla-room approve` | At phase gates, advances to next phase |
| **Reject** | `vanilla-room reject "reason"` | Sends rejection, triggers reflexion |
| **Recruit** | `vanilla-room recruit security` | Adds agent to room mid-task |
| **Eject** | `vanilla-room eject tester` | Removes agent from queue |
| **Status** | `vanilla-room status` | Shows phase, queue, approvals |

User messages appear in the transcript as `[User]` and are treated with highest priority by the Conductor.

## Communication Protocol

Agents communicate through structured tags in their natural language responses:

```
[STATUS: APPROVED]          — vote to advance
[STATUS: REJECTED]          — vote to reject (triggers reflexion)
[STATUS: DISCUSSING]        — not ready to vote yet
[STATUS: BLOCKING]          — blocks phase advancement

[HANDOFF: Developer]        — request Developer speaks next
[HANDOFF: Reviewer, Tester] — request multiple agents

[DECISION: Use JWT with 1h expiry, no refresh tokens]
                            — logged to decision log

[ARTIFACT: design.md]       — declares an artifact was produced

[RECRUIT: database_expert]  — request to bring in a specialist
[DEBOARD: researcher]       — request to remove an agent
```

These tags are parsed by the Conductor. The rest of the message is free-form natural language that other agents read in the transcript.

## Context per Agent Turn

Each agent turn receives:

```
┌─────────────────────────────────────────────────┐
│ SYSTEM PROMPT                                    │
│                                                  │
│ You are {name}, a {role}.                        │
│ {personality}                                    │
│                                                  │
│ RULES:                                           │
│ - Read the transcript and respond naturally      │
│ - Vote with [STATUS: X] when ready               │
│ - Use [HANDOFF: X] to request next speaker       │
│ - Use [DECISION: X] to log decisions             │
│ - You can REJECT work that doesn't meet your     │
│   standards — the room will fix it               │
│ - Be direct. Don't be polite for politeness sake │
│                                                  │
│ CURRENT STATE:                                   │
│ Phase: {phase} | Step: {step}                    │
│ Artifacts: {list}                                │
│ Approvals: Architect✓ Developer? Reviewer?       │
│ Your branch: vr/{your_name}                      │
├─────────────────────────────────────────────────┤
│ PROJECT BRIEF                                    │
│ {brief}                                          │
├─────────────────────────────────────────────────┤
│ TRANSCRIPT (last 30 messages)                    │
│ [Conductor] Objective: Add JWT auth...           │
│ [Architect] I propose...                         │
│ [Security] Concern about...                      │
│ ...                                              │
├─────────────────────────────────────────────────┤
│ DECISIONS SO FAR                                 │
│ - Use JWT with 1h expiry                         │
│ - Secret from env var                            │
├─────────────────────────────────────────────────┤
│ YOUR TURN. Respond naturally.                    │
└─────────────────────────────────────────────────┘
```

Estimated tokens per turn: ~5-10K input, ~1K output.
Typical task: ~15-25 turns = ~100-200K total tokens.
On MAX plan: **free**.

## HELIX Integration (Future)

HELIX can watch `.vanilla-room/` and display:

```
┌─ Vanilla Room: add-jwt-auth ● implementing ──────┐
│ 🟢 Architect    APPROVED                          │
│ 🟡 Developer    WORKING   ← current               │
│ ⚪ Reviewer     WAITING                           │
│ ⚪ Tester       WAITING                           │
│ Step: implement (3/6) | Turns: 12                 │
├───────────────────────────────────────────────────┤
│ [Architect] Simple JWT with 1h expiry, no...      │
│ [Security] Store secret in env var, add rate...   │
│ [Conductor] Consensus. Developer, implement.      │
│ [Developer] Working on vr/developer...            │
└───────────────────────────────────────────────────┘
```

## File Structure

```
.vanilla-room/                    ← created in user's project root
├── brief.md                      ← the task description
├── roster.json                   ← current team
├── state.json                    ← phase, step, approvals, artifacts
├── discussion.jsonl              ← transcript (append-only)
├── decisions.json                ← logged decisions
├── playbook.toml                 ← active playbook
└── artifacts/                    ← produced outputs
    ├── design.md
    ├── test_results.md
    └── review_feedback.md
```

## Error Handling

- **Agent goes off-track:** Conductor detects via missing [STATUS] tags, re-prompts with clearer instructions
- **Agent produces bad code:** Reviewer/Tester reject, reflexion loop fixes it
- **Infinite reflexion loop:** Max 3 rejection rounds, then pause for user input
- **Agent hallucination:** Code is on a branch — no damage to main. Reviewer catches it.
- **Network/API failure:** Conductor retries with exponential backoff, pauses after 3 failures
- **User disappears:** Room auto-pauses at phase gates requiring user approval

## Security

- Agents NEVER touch the `main` branch directly
- All work happens on `vr/*` branches
- User must explicitly approve the final merge
- `.env` files and secrets are excluded from agent context
- Agent tool access is role-restricted (Reviewer can't Edit files)
