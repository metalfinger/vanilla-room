# Vanilla Room — Project Instructions

## What This Is
Vanilla Room is an autonomous multi-agent orchestration CLI for software engineering. AI agents collaborate in a shared "room" — they debate, code on git branches, review diffs, and reach consensus. Built in Rust, powered by Claude Code CLI (`claude -p`).

## Who I Am
- Hiren Kangad — game developer & creative technologist
- I prefer direct action over long explanations — do the thing, don't explain
- Short, clear responses. Skip preamble
- When I say "fix it" — fix it
- Commit messages: `feat:`, `fix:`, `docs:`, `build:`, `test:`, `chore:`

## Project Rules
- Language: Rust (4-space indentation)
- Build: `cargo build --release`
- Binary: `vanilla-room`
- CLI framework: clap with derive macros
- Serialization: serde + serde_json + toml
- Git user: Hiren Kangad <hir.012612@gmail.com>
- Default branch: main
- Always create new commits, never amend unless I explicitly ask
- autocrlf is on (Windows)

## Architecture
- Read `docs/DESIGN.md` for full architecture
- Read `docs/PLAN.md` for the implementation plan (14 tasks)
- The implementation plan has detailed file-by-file instructions — follow it

## Key Design Decisions
- Agents are `claude -p` calls with personas (one-shot, free on MAX plan)
- Communication via shared JSONL transcript (.vanilla-room/discussion.jsonl)
- Git branches per agent for code isolation (vr/developer, vr/tester, etc.)
- Conductor is deterministic Rust code (NOT an AI agent)
- Smart recruiting: analyze task → pick team + playbook via claude -p
- Reflexion loops: rejection locks queue to fixer+rejector until resolved
- Personas in TOML files (personas/*.toml)
- Playbooks in TOML files (playbooks/*.toml)

## Files to Never Touch
- `.env` files — never edit, never commit, never read contents
- User's repo files outside of vr/* branches

## How to Start
Run: `implement the plan at docs/PLAN.md`
Start with Task 1 (types) + Task 13 (Cargo.toml), then work through sequentially.
