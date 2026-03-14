mod types;
pub mod agent;
pub mod conductor;
pub mod context;
pub mod discussion;
pub mod git_workspace;
pub mod parser;
pub mod persona;
pub mod playbook;
pub mod recruiter;
pub mod state;

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use chrono::Utc;
use clap::{Parser, Subcommand};
use uuid::Uuid;

use crate::conductor::Conductor;
use crate::discussion::Discussion;
use crate::git_workspace::GitWorkspace;
use crate::state::State;
use crate::types::*;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "vanilla-room", about = "Multi-agent orchestration CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new vanilla-room session
    Init {
        /// The task description
        task: String,
        /// Path to the repository (default: current directory)
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Override the playbook (skip recruiter selection)
        #[arg(long)]
        playbook: Option<String>,
    },
    /// Run the conductor loop
    Run,
    /// Show current session status
    Status,
    /// Add a user message to the transcript
    Say {
        /// The message to add
        message: String,
    },
    /// Approve the current step
    Approve,
    /// Reject the current step with a reason
    Reject {
        /// Reason for rejection
        reason: String,
    },
    /// Add an agent to the roster
    Recruit {
        /// Agent persona name
        agent_name: String,
    },
    /// Remove an agent from the roster
    Eject {
        /// Agent persona name
        agent_name: String,
    },
    /// Pause the conductor
    Pause,
    /// Resume the conductor
    Resume,
    /// Print the discussion transcript
    Transcript {
        /// Show only the last N entries
        #[arg(long)]
        last: Option<usize>,
    },
    /// Clean up session data and branches
    Cleanup,
}

// ---------------------------------------------------------------------------
// Directory resolution helpers
// ---------------------------------------------------------------------------

fn find_resource_dir(name: &str) -> PathBuf {
    // 1. Next to the executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join(name);
            if candidate.is_dir() {
                return candidate;
            }
        }
    }

    // 2. Current directory
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let candidate = cwd.join(name);
    if candidate.is_dir() {
        return candidate;
    }

    // 3. Fail with helpful error
    eprintln!(
        "Error: Could not find '{}/' directory. Looked next to the executable and in the current directory.",
        name
    );
    process::exit(1);
}

fn vr_dir(repo: &Path) -> PathBuf {
    repo.join(".vanilla-room")
}

fn require_vr_dir(repo: &Path) -> PathBuf {
    let dir = vr_dir(repo);
    if !dir.is_dir() {
        eprintln!("Error: No .vanilla-room/ directory found. Run 'vanilla-room init' first.");
        process::exit(1);
    }
    dir
}

fn repo_dir(repo_arg: Option<PathBuf>) -> PathBuf {
    repo_arg.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Load session_id from state.json's RoomConfig or from a stored file.
fn load_session_id(vr: &Path) -> String {
    let config_path = vr.join("config.json");
    if config_path.exists() {
        if let Ok(contents) = fs::read_to_string(&config_path) {
            if let Ok(config) = serde_json::from_str::<RoomConfig>(&contents) {
                return config.session_id;
            }
        }
    }
    eprintln!("Error: Could not load session config from .vanilla-room/config.json");
    process::exit(1);
}

fn load_roster(vr: &Path) -> Roster {
    let path = vr.join("roster.json");
    let contents = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Error: Could not read roster.json: {}", e);
        process::exit(1);
    });
    serde_json::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Error: Could not parse roster.json: {}", e);
        process::exit(1);
    })
}

fn load_playbook_from_vr(vr: &Path) -> Playbook {
    let path = vr.join("playbook.toml");
    let contents = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Error: Could not read playbook.toml: {}", e);
        process::exit(1);
    });
    toml::from_str(&contents).unwrap_or_else(|e| {
        eprintln!("Error: Could not parse playbook.toml: {}", e);
        process::exit(1);
    })
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

fn cmd_init(task: String, repo_arg: Option<PathBuf>, playbook_override: Option<String>) {
    let repo = repo_dir(repo_arg);
    let personas_dir = find_resource_dir("personas");
    let playbooks_dir = find_resource_dir("playbooks");

    let vr = vr_dir(&repo);
    if vr.is_dir() {
        eprintln!("Error: Session already exists. Run 'vanilla-room cleanup' first.");
        process::exit(1);
    }
    fs::create_dir_all(&vr).unwrap_or_else(|e| {
        eprintln!("Error: Could not create .vanilla-room/: {}", e);
        process::exit(1);
    });

    // Save brief
    fs::write(vr.join("brief.md"), &task).unwrap_or_else(|e| {
        eprintln!("Error: Could not write brief.md: {}", e);
        process::exit(1);
    });

    // Recruit team
    println!("Analyzing task and recruiting team...");
    let plan = recruiter::recruit(&task, &personas_dir, &playbooks_dir);

    // Use override playbook if provided
    let playbook_name = playbook_override.unwrap_or(plan.playbook.clone());

    // Load playbook
    let pb = playbook::load_playbook(&playbooks_dir, &playbook_name).unwrap_or_else(|e| {
        eprintln!("Error: Could not load playbook '{}': {}", playbook_name, e);
        process::exit(1);
    });

    // Load agent profiles
    let mut agents: Vec<AgentProfile> = Vec::new();
    for agent_name in &plan.agents {
        match persona::load_persona(&personas_dir, agent_name) {
            Ok(profile) => agents.push(profile),
            Err(e) => {
                eprintln!("Warning: Could not load persona '{}': {}", agent_name, e);
            }
        }
    }

    if agents.is_empty() {
        eprintln!("Error: No agents could be loaded. Check your personas/ directory.");
        process::exit(1);
    }

    let roster = Roster(agents);

    // Save roster
    let roster_json = serde_json::to_string_pretty(&roster).unwrap();
    fs::write(vr.join("roster.json"), &roster_json).unwrap_or_else(|e| {
        eprintln!("Error: Could not write roster.json: {}", e);
        process::exit(1);
    });

    // Generate session ID
    let session_id = Uuid::new_v4().to_string();

    // Capture original branch
    let original_branch = {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap_or_else(|e| {
                eprintln!("Error: git not found: {}", e);
                process::exit(1);
            });
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };

    // Save config
    let config = RoomConfig {
        project_dir: repo.clone(),
        repo_dir: repo.clone(),
        session_id: session_id.clone(),
        original_branch: original_branch.clone(),
    };
    let config_json = serde_json::to_string_pretty(&config).unwrap();
    fs::write(vr.join("config.json"), &config_json).unwrap_or_else(|e| {
        eprintln!("Error: Could not write config.json: {}", e);
        process::exit(1);
    });

    // Save initial state
    let _state = State::new(vr.join("state.json")).unwrap_or_else(|e| {
        eprintln!("Error: Could not create state.json: {}", e);
        process::exit(1);
    });

    // Copy playbook to .vanilla-room/
    let pb_source = playbooks_dir.join(format!("{}.toml", playbook_name));
    fs::copy(&pb_source, vr.join("playbook.toml")).unwrap_or_else(|e| {
        eprintln!("Error: Could not copy playbook: {}", e);
        process::exit(1);
    });

    // Create git branches
    let git = GitWorkspace::new(repo.clone(), &session_id).unwrap_or_else(|e| {
        eprintln!("Error: Could not initialize git workspace: {}", e);
        process::exit(1);
    });

    git.create_session_branch().unwrap_or_else(|e| {
        eprintln!("Error: Could not create session branch: {}", e);
        process::exit(1);
    });

    for agent in &roster.0 {
        git.create_agent_branch(&agent.name).unwrap_or_else(|e| {
            eprintln!(
                "Error: Could not create branch for '{}': {}",
                agent.name, e
            );
            process::exit(1);
        });
    }

    // Go back to session branch
    let _ = git.restore_original_branch();

    // Print summary
    println!();
    println!("=== Vanilla Room Initialized ===");
    println!();
    println!("Task: {}", task);
    println!("Playbook: {} - {}", pb.name, pb.description);
    println!("Session: {}", session_id);
    println!();
    println!("Team:");
    for agent in &roster.0 {
        println!("  - {} ({})", agent.name, agent.role);
    }
    println!();
    println!("Recruiter reasoning: {}", plan.reasoning);
    println!();
    println!("Ready to run. Use 'vanilla-room run' to start.");
}

fn cmd_run() {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = require_vr_dir(&repo);
    let personas_dir = find_resource_dir("personas");
    let playbooks_dir = find_resource_dir("playbooks");

    let config = {
        let config_path = vr.join("config.json");
        let contents = fs::read_to_string(&config_path).unwrap_or_else(|e| {
            eprintln!("Error: Could not read config.json: {}", e);
            process::exit(1);
        });
        serde_json::from_str::<RoomConfig>(&contents).unwrap_or_else(|e| {
            eprintln!("Error: Could not parse config.json: {}", e);
            process::exit(1);
        })
    };

    let state = State::new(vr.join("state.json")).unwrap_or_else(|e| {
        eprintln!("Error: Could not load state: {}", e);
        process::exit(1);
    });
    if state.data.phase == Phase::Complete {
        println!("Session already complete. Run 'vanilla-room cleanup' to start fresh.");
        return;
    }

    let roster = load_roster(&vr);
    let pb = load_playbook_from_vr(&vr);

    let mut conductor =
        Conductor::new(config, roster, pb, personas_dir, playbooks_dir).unwrap_or_else(|e| {
            eprintln!("Error: Could not create conductor: {}", e);
            process::exit(1);
        });

    println!("Starting conductor loop...");
    println!();

    match conductor.run() {
        Ok(result) => {
            println!();
            match result {
                conductor::TurnResult::Complete => println!("Room complete."),
                conductor::TurnResult::Paused => {
                    println!("Room paused. Use 'vanilla-room resume' to continue.")
                }
                conductor::TurnResult::Error(msg) => {
                    eprintln!("Room error: {}", msg);
                    process::exit(1);
                }
                conductor::TurnResult::Continue => {
                    // Should not happen as final result
                }
            }
        }
        Err(e) => {
            eprintln!("Error: Conductor failed: {}", e);
            process::exit(1);
        }
    }
}

fn cmd_status() {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = require_vr_dir(&repo);

    let state = State::new(vr.join("state.json")).unwrap_or_else(|e| {
        eprintln!("Error: Could not load state: {}", e);
        process::exit(1);
    });

    let roster = load_roster(&vr);
    let session_id = load_session_id(&vr);

    println!("=== Vanilla Room Status ===");
    println!();
    println!("Session: {}", session_id);
    println!("Phase: {}", state.data.phase);
    if let Some(ref step) = state.data.current_step_id {
        println!("Current step: {}", step);
    } else {
        println!("Current step: (none)");
    }
    println!();

    println!("Roster:");
    for agent in &roster.0 {
        let vote = state
            .data
            .approvals
            .get(&agent.name)
            .map(|v| format!("{}", v))
            .unwrap_or_else(|| "-".to_string());
        println!("  {} ({}) - {}", agent.name, agent.role, vote);
    }
    println!();

    if !state.data.decision_log.is_empty() {
        println!("Decisions:");
        for d in &state.data.decision_log {
            println!("  - {}", d);
        }
        println!();
    }

    if !state.data.artifacts.is_empty() {
        println!("Artifacts:");
        for (name, owner) in &state.data.artifacts {
            println!("  {} (by {})", name, owner);
        }
    }
}

fn cmd_say(message: String) {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = require_vr_dir(&repo);

    let discussion = Discussion::new(vr.join("discussion.jsonl")).unwrap_or_else(|e| {
        eprintln!("Error: Could not open discussion log: {}", e);
        process::exit(1);
    });

    let entry = DiscussionEntry {
        timestamp: Utc::now(),
        agent_name: "[User]".to_string(),
        role: "user".to_string(),
        content: message,
        status_vote: None,
        handoff_targets: vec![],
        decisions: vec![],
        artifacts: vec![],
    };

    discussion.append(&entry).unwrap_or_else(|e| {
        eprintln!("Error: Could not append message: {}", e);
        process::exit(1);
    });

    println!("Message added.");
}

fn cmd_approve() {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = require_vr_dir(&repo);

    let mut state = State::new(vr.join("state.json")).unwrap_or_else(|e| {
        eprintln!("Error: Could not load state: {}", e);
        process::exit(1);
    });

    state
        .record_vote("[User]", Vote::Approved)
        .unwrap_or_else(|e| {
            eprintln!("Error: Could not record vote: {}", e);
            process::exit(1);
        });

    // Add approval to discussion
    let discussion = Discussion::new(vr.join("discussion.jsonl")).unwrap_or_else(|e| {
        eprintln!("Error: Could not open discussion log: {}", e);
        process::exit(1);
    });

    let entry = DiscussionEntry {
        timestamp: Utc::now(),
        agent_name: "[User]".to_string(),
        role: "user".to_string(),
        content: "Approved.".to_string(),
        status_vote: Some(Vote::Approved),
        handoff_targets: vec![],
        decisions: vec![],
        artifacts: vec![],
    };

    discussion.append(&entry).unwrap_or_else(|e| {
        eprintln!("Error: Could not append approval: {}", e);
        process::exit(1);
    });

    println!("Approval recorded.");

    // Check if we're at a user_approval gate
    let pb = load_playbook_from_vr(&vr);
    if let Some(ref step_id) = state.data.current_step_id {
        if let Some(step) = playbook::current_step(&pb, step_id) {
            if step.gate.as_deref() == Some("user_approval") {
                // Advance to next step
                if let Some(next) = playbook::next_step(&pb, step_id) {
                    let next_id = next.id.clone();
                    state.data.current_step_id = Some(next_id.clone());
                    for vote in state.data.approvals.values_mut() {
                        *vote = Vote::Pending;
                    }
                    state.save().unwrap_or_else(|e| {
                        eprintln!("Error: Could not save state: {}", e);
                        process::exit(1);
                    });
                    println!("Advanced to step: {}", next_id);
                } else {
                    state.data.current_step_id = None;
                    state.data.phase = Phase::Complete;
                    state.save().unwrap_or_else(|e| {
                        eprintln!("Error: Could not save state: {}", e);
                        process::exit(1);
                    });
                    println!("All steps complete.");
                }
            }
        }
    }
}

fn cmd_reject(reason: String) {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = require_vr_dir(&repo);

    let mut state = State::new(vr.join("state.json")).unwrap_or_else(|e| {
        eprintln!("Error: Could not load state: {}", e);
        process::exit(1);
    });

    state
        .record_vote("[User]", Vote::Rejected)
        .unwrap_or_else(|e| {
            eprintln!("Error: Could not record vote: {}", e);
            process::exit(1);
        });

    let discussion = Discussion::new(vr.join("discussion.jsonl")).unwrap_or_else(|e| {
        eprintln!("Error: Could not open discussion log: {}", e);
        process::exit(1);
    });

    let entry = DiscussionEntry {
        timestamp: Utc::now(),
        agent_name: "[User]".to_string(),
        role: "user".to_string(),
        content: format!("Rejected: {}", reason),
        status_vote: Some(Vote::Rejected),
        handoff_targets: vec![],
        decisions: vec![],
        artifacts: vec![],
    };

    discussion.append(&entry).unwrap_or_else(|e| {
        eprintln!("Error: Could not append rejection: {}", e);
        process::exit(1);
    });

    println!("Rejection recorded. Reason: {}", reason);
}

fn cmd_recruit(agent_name: String) {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = require_vr_dir(&repo);
    let personas_dir = find_resource_dir("personas");

    let profile = persona::load_persona(&personas_dir, &agent_name).unwrap_or_else(|e| {
        eprintln!("Error: Could not load persona '{}': {}", agent_name, e);
        process::exit(1);
    });

    let mut roster = load_roster(&vr);

    if roster.0.iter().any(|a| a.name == profile.name) {
        println!("Agent '{}' is already on the roster.", profile.name);
        return;
    }

    roster.0.push(profile.clone());

    let roster_json = serde_json::to_string_pretty(&roster).unwrap();
    fs::write(vr.join("roster.json"), &roster_json).unwrap_or_else(|e| {
        eprintln!("Error: Could not write roster.json: {}", e);
        process::exit(1);
    });

    // Create git branch for new agent
    let session_id = load_session_id(&vr);
    let config_path = vr.join("config.json");
    if let Ok(contents) = fs::read_to_string(&config_path) {
        if let Ok(config) = serde_json::from_str::<RoomConfig>(&contents) {
            let orig = if config.original_branch.is_empty() {
                "main".to_string()
            } else {
                config.original_branch
            };
            let git = GitWorkspace::new_with_original(repo.clone(), &session_id, orig);
            let _ = git.ensure_agent_branch(&profile.name);
        }
    }

    println!("Recruited {} ({}).", profile.name, profile.role);
}

fn cmd_eject(agent_name: String) {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = require_vr_dir(&repo);

    let mut roster = load_roster(&vr);
    let before = roster.0.len();
    roster.0.retain(|a| a.name != agent_name);

    if roster.0.len() == before {
        println!("Agent '{}' not found on the roster.", agent_name);
        return;
    }

    let roster_json = serde_json::to_string_pretty(&roster).unwrap();
    fs::write(vr.join("roster.json"), &roster_json).unwrap_or_else(|e| {
        eprintln!("Error: Could not write roster.json: {}", e);
        process::exit(1);
    });

    println!("Ejected '{}'.", agent_name);
}

fn cmd_pause() {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = require_vr_dir(&repo);

    let mut state = State::new(vr.join("state.json")).unwrap_or_else(|e| {
        eprintln!("Error: Could not load state: {}", e);
        process::exit(1);
    });

    state.data.phase = Phase::Paused;
    state.save().unwrap_or_else(|e| {
        eprintln!("Error: Could not save state: {}", e);
        process::exit(1);
    });

    println!("Room paused.");
}

fn cmd_resume() {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = require_vr_dir(&repo);
    let personas_dir = find_resource_dir("personas");
    let playbooks_dir = find_resource_dir("playbooks");

    let config = {
        let config_path = vr.join("config.json");
        let contents = fs::read_to_string(&config_path).unwrap_or_else(|e| {
            eprintln!("Error: Could not read config.json: {}", e);
            process::exit(1);
        });
        serde_json::from_str::<RoomConfig>(&contents).unwrap_or_else(|e| {
            eprintln!("Error: Could not parse config.json: {}", e);
            process::exit(1);
        })
    };

    let roster = load_roster(&vr);
    let pb = load_playbook_from_vr(&vr);

    let mut conductor =
        Conductor::new(config, roster, pb, personas_dir, playbooks_dir).unwrap_or_else(|e| {
            eprintln!("Error: Could not create conductor: {}", e);
            process::exit(1);
        });

    println!("Resuming conductor loop...");
    println!();

    match conductor.resume() {
        Ok(result) => {
            println!();
            match result {
                conductor::TurnResult::Complete => println!("Room complete."),
                conductor::TurnResult::Paused => {
                    println!("Room paused. Use 'vanilla-room resume' to continue.")
                }
                conductor::TurnResult::Error(msg) => {
                    eprintln!("Room error: {}", msg);
                    process::exit(1);
                }
                conductor::TurnResult::Continue => {}
            }
        }
        Err(e) => {
            eprintln!("Error: Conductor failed: {}", e);
            process::exit(1);
        }
    }
}

fn cmd_transcript(last: Option<usize>) {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = require_vr_dir(&repo);

    let discussion = Discussion::new(vr.join("discussion.jsonl")).unwrap_or_else(|e| {
        eprintln!("Error: Could not open discussion log: {}", e);
        process::exit(1);
    });

    let entries = match last {
        Some(n) => discussion.read_last(n).unwrap_or_else(|e| {
            eprintln!("Error: Could not read transcript: {}", e);
            process::exit(1);
        }),
        None => discussion.read_all().unwrap_or_else(|e| {
            eprintln!("Error: Could not read transcript: {}", e);
            process::exit(1);
        }),
    };

    if entries.is_empty() {
        println!("(no entries)");
        return;
    }

    println!("{}", Discussion::format_for_prompt(&entries));
}

fn cmd_cleanup() {
    let repo = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let vr = vr_dir(&repo);

    if vr.is_dir() {
        // Load session_id before deleting
        let session_id = {
            let config_path = vr.join("config.json");
            if config_path.exists() {
                fs::read_to_string(&config_path)
                    .ok()
                    .and_then(|c| serde_json::from_str::<RoomConfig>(&c).ok())
                    .map(|c| c.session_id)
            } else {
                None
            }
        };

        // Clean up git branches
        if let Some(ref sid) = session_id {
            if let Ok(git) = GitWorkspace::new(repo.clone(), sid) {
                let _ = git.cleanup_branches(sid);
            }
        }

        // Delete .vanilla-room/
        fs::remove_dir_all(&vr).unwrap_or_else(|e| {
            eprintln!("Error: Could not remove .vanilla-room/: {}", e);
            process::exit(1);
        });

        println!("Cleaned up .vanilla-room/ and vr/* branches.");
    } else {
        println!("Nothing to clean up.");
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            task,
            repo,
            playbook,
        } => cmd_init(task, repo, playbook),
        Commands::Run => cmd_run(),
        Commands::Status => cmd_status(),
        Commands::Say { message } => cmd_say(message),
        Commands::Approve => cmd_approve(),
        Commands::Reject { reason } => cmd_reject(reason),
        Commands::Recruit { agent_name } => cmd_recruit(agent_name),
        Commands::Eject { agent_name } => cmd_eject(agent_name),
        Commands::Pause => cmd_pause(),
        Commands::Resume => cmd_resume(),
        Commands::Transcript { last } => cmd_transcript(last),
        Commands::Cleanup => cmd_cleanup(),
    }
}
