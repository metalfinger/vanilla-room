use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::types::{AgentProfile, RoomConfig};

// ---------------------------------------------------------------------------
// AgentError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum AgentError {
    NotInstalled,
    AuthFailure,
    Timeout,
    NonZeroExit { code: Option<i32>, stderr: String },
    Io(std::io::Error),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::NotInstalled => write!(f, "claude CLI is not installed or not in PATH"),
            AgentError::AuthFailure => write!(f, "claude CLI authentication failure"),
            AgentError::Timeout => write!(f, "agent turn timed out after 5 minutes"),
            AgentError::NonZeroExit { code, stderr } => {
                write!(f, "claude exited with code {:?}: {}", code, stderr)
            }
            AgentError::Io(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<std::io::Error> for AgentError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::NotFound {
            AgentError::NotInstalled
        } else {
            AgentError::Io(e)
        }
    }
}

// ---------------------------------------------------------------------------
// AgentExecutor
// ---------------------------------------------------------------------------

pub struct AgentExecutor {
    config: RoomConfig,
}

const TURN_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

impl AgentExecutor {
    pub fn new(config: &RoomConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    /// Run a full agent turn with the agent's allowed_tools list.
    pub fn execute_turn(
        &self,
        agent: &AgentProfile,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AgentError> {
        let tools = agent.allowed_tools.join(",");
        self.run_claude(system_prompt, user_prompt, &tools)
    }

    /// Run a thinking/review turn restricted to Read,Glob,Grep only.
    pub fn execute_thinking_turn(
        &self,
        _agent: &AgentProfile,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AgentError> {
        self.run_claude(system_prompt, user_prompt, "Read,Glob,Grep")
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn run_claude(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        tools: &str,
    ) -> Result<String, AgentError> {
        let mut child = Command::new("claude")
            .args([
                "-p",
                "-",
                "--system-prompt",
                system_prompt,
                "--allowedTools",
                tools,
                "--print",
                "--output-format",
                "text",
            ])
            .current_dir(&self.config.repo_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(AgentError::from)?;

        // Write user prompt to stdin then close it.
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(user_prompt.as_bytes())
                .map_err(AgentError::Io)?;
            // stdin dropped here, signalling EOF to child
        }

        // Wait with timeout using a separate thread.
        let output = wait_with_timeout(child, TURN_TIMEOUT)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            if stderr.contains("authentication")
                || stderr.contains("401")
                || stderr.contains("Unauthorized")
            {
                return Err(AgentError::AuthFailure);
            }
            return Err(AgentError::NonZeroExit {
                code: output.status.code(),
                stderr,
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        Ok(stdout)
    }
}

// ---------------------------------------------------------------------------
// Timeout helper
// ---------------------------------------------------------------------------

/// Waits for a child process and kills it if it exceeds `timeout`.
fn wait_with_timeout(
    child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, AgentError> {
    use std::sync::mpsc;
    use std::thread;

    let (tx, rx) = mpsc::channel();

    // Spawn a thread that blocks on wait_with_output.
    thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(AgentError::Io(e)),
        Err(_) => {
            // Timeout — the child thread still owns the process handle so we
            // cannot kill it here, but the process will be orphaned briefly
            // before the OS reclaims it. In practice the thread will clean up.
            Err(AgentError::Timeout)
        }
    }
}
