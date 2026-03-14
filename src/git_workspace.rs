use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct GitWorkspace {
    pub repo_dir: PathBuf,
    pub session_id: String,
    pub original_branch: String,
}

fn run_git(repo_dir: &Path, args: &[&str]) -> io::Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let msg = if stderr.is_empty() { stdout } else { stderr };
        Err(io::Error::new(io::ErrorKind::Other, msg))
    }
}

impl GitWorkspace {
    pub fn new(repo_dir: PathBuf, session_id: &str) -> io::Result<Self> {
        let original_branch = run_git(&repo_dir, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        Ok(Self {
            repo_dir,
            session_id: session_id.to_string(),
            original_branch,
        })
    }

    pub fn session_branch_name(&self) -> String {
        format!("vr/session-{}", self.session_id)
    }

    pub fn agent_branch_name(agent_name: &str) -> String {
        format!("vr/{}", agent_name.to_lowercase())
    }

    pub fn create_session_branch(&self) -> io::Result<()> {
        let branch = self.session_branch_name();
        run_git(&self.repo_dir, &["checkout", "-b", &branch])?;
        Ok(())
    }

    pub fn create_agent_branch(&self, agent_name: &str) -> io::Result<()> {
        let session_branch = self.session_branch_name();
        let agent_branch = Self::agent_branch_name(agent_name);
        run_git(
            &self.repo_dir,
            &["checkout", "-b", &agent_branch, &session_branch],
        )?;
        Ok(())
    }

    pub fn checkout_agent_branch(&self, agent_name: &str) -> io::Result<()> {
        let branch = Self::agent_branch_name(agent_name);
        run_git(&self.repo_dir, &["checkout", &branch])?;
        Ok(())
    }

    pub fn commit_agent_work(&self, agent_name: &str, message: &str) -> io::Result<()> {
        self.checkout_agent_branch(agent_name)?;

        let status = run_git(&self.repo_dir, &["status", "--porcelain"])?;
        if status.is_empty() {
            return Ok(());
        }

        run_git(&self.repo_dir, &["add", "-A"])?;
        run_git(&self.repo_dir, &["commit", "-m", message])?;
        Ok(())
    }

    pub fn get_diff(&self, agent_name: &str) -> String {
        let session_branch = self.session_branch_name();
        let agent_branch = Self::agent_branch_name(agent_name);
        let range = format!("{}...{}", session_branch, agent_branch);
        run_git(&self.repo_dir, &["diff", &range])
            .unwrap_or_else(|e| format!("error getting diff: {}", e))
    }

    pub fn merge_agent_to_session(&self, agent_name: &str) -> io::Result<()> {
        let session_branch = self.session_branch_name();
        let agent_branch = Self::agent_branch_name(agent_name);

        run_git(&self.repo_dir, &["checkout", &session_branch])?;
        run_git(
            &self.repo_dir,
            &["merge", "--no-ff", &agent_branch, "-m", &format!("merge {}", agent_branch)],
        )?;
        Ok(())
    }

    pub fn merge_session_to_main(&self) -> io::Result<()> {
        let session_branch = self.session_branch_name();

        // Merge back to whatever branch the user was on when they started
        run_git(&self.repo_dir, &["checkout", &self.original_branch])?;
        run_git(
            &self.repo_dir,
            &["merge", "--no-ff", &session_branch, "-m", &format!("merge {}", session_branch)],
        )?;
        Ok(())
    }

    pub fn cleanup_branches(&self, session_id: &str) -> io::Result<()> {
        let session_prefix = format!("vr/session-{}", session_id);
        let list_output = run_git(&self.repo_dir, &["branch", "--list", "vr/*"])?;

        self.restore_original_branch().ok();

        // Collect agent branches that belong to this session.
        // Session branch: vr/session-{id}
        // Agent branches: vr/{agent_name} — we track which ones we created
        // by checking the roster, but since we don't have it here, delete
        // the session branch and any agent branches that were forked from it.
        let mut to_delete = Vec::new();
        for line in list_output.lines() {
            let branch = line.trim().trim_start_matches('*').trim();
            if branch == session_prefix {
                to_delete.push(branch.to_string());
            } else if branch.starts_with("vr/") && branch != session_prefix {
                // Check if this agent branch was based on our session branch
                let merge_base = run_git(
                    &self.repo_dir,
                    &["merge-base", "--is-ancestor", &session_prefix, branch],
                );
                if merge_base.is_ok() {
                    to_delete.push(branch.to_string());
                }
            }
        }

        // Delete agent branches first, then session branch
        for branch in &to_delete {
            run_git(&self.repo_dir, &["branch", "-D", branch]).ok();
        }
        Ok(())
    }

    pub fn restore_original_branch(&self) -> io::Result<()> {
        run_git(&self.repo_dir, &["checkout", &self.original_branch])?;
        Ok(())
    }

    pub fn current_branch(&self) -> io::Result<String> {
        run_git(&self.repo_dir, &["rev-parse", "--abbrev-ref", "HEAD"])
    }
}
