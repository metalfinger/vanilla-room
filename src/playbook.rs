use std::path::Path;

use crate::types::Playbook;
use crate::types::PlaybookStep;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum PlaybookError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Message(String),
}

impl std::fmt::Display for PlaybookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaybookError::Io(e) => write!(f, "IO error: {}", e),
            PlaybookError::Parse(e) => write!(f, "TOML parse error: {}", e),
            PlaybookError::Message(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for PlaybookError {}

impl From<std::io::Error> for PlaybookError {
    fn from(e: std::io::Error) -> Self {
        PlaybookError::Io(e)
    }
}

impl From<toml::de::Error> for PlaybookError {
    fn from(e: toml::de::Error) -> Self {
        PlaybookError::Parse(e)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load a playbook by name from `playbooks_dir/{name}.toml`.
pub fn load_playbook(playbooks_dir: &Path, name: &str) -> Result<Playbook, PlaybookError> {
    let path = playbooks_dir.join(format!("{}.toml", name));
    let contents = std::fs::read_to_string(&path).map_err(|e| {
        PlaybookError::Message(format!(
            "failed to read playbook file '{}': {}",
            path.display(),
            e
        ))
    })?;
    let playbook: Playbook = toml::from_str(&contents).map_err(|e| {
        PlaybookError::Message(format!(
            "failed to parse playbook file '{}': {}",
            path.display(),
            e
        ))
    })?;
    Ok(playbook)
}

/// Find a step by its id.
pub fn current_step<'a>(playbook: &'a Playbook, step_id: &str) -> Option<&'a PlaybookStep> {
    playbook.steps.iter().find(|s| s.id == step_id)
}

/// Get the step that follows the given step_id, using the `next` field.
pub fn next_step<'a>(playbook: &'a Playbook, step_id: &str) -> Option<&'a PlaybookStep> {
    let current = current_step(playbook, step_id)?;
    let next_id = current.next.as_deref()?;
    current_step(playbook, next_id)
}

/// Get the `required_role` for a step.
pub fn required_role(playbook: &Playbook, step_id: &str) -> Option<String> {
    current_step(playbook, step_id).map(|s| s.required_role.clone())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    const SAMPLE_TOML: &str = r#"
name = "sample_playbook"
description = "A sample playbook for testing"

[[steps]]
id = "alpha"
description = "First step"
required_role = "Architect"
output_artifact = "alpha.md"
next = "beta"

[[steps]]
id = "beta"
description = "Second step"
required_role = "Developer"
gate = "unanimous_approval"
next = "gamma"

[[steps]]
id = "gamma"
description = "Final step"
required_role = "Conductor"
"#;

    fn write_playbook(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(format!("{}.toml", name)), content).unwrap();
    }

    #[test]
    fn load_playbook_parses_fields() {
        let dir = tempdir().unwrap();
        write_playbook(dir.path(), "sample", SAMPLE_TOML);

        let pb = load_playbook(dir.path(), "sample").unwrap();
        assert_eq!(pb.name, "sample_playbook");
        assert_eq!(pb.description, "A sample playbook for testing");
        assert_eq!(pb.steps.len(), 3);
        assert_eq!(pb.steps[0].id, "alpha");
        assert_eq!(pb.steps[0].required_role, "Architect");
        assert_eq!(pb.steps[0].output_artifact.as_deref(), Some("alpha.md"));
        assert_eq!(pb.steps[0].next.as_deref(), Some("beta"));
        assert_eq!(pb.steps[1].gate.as_deref(), Some("unanimous_approval"));
        assert!(pb.steps[2].next.is_none());
    }

    #[test]
    fn load_playbook_missing_file_returns_error() {
        let dir = tempdir().unwrap();
        let result = load_playbook(dir.path(), "nonexistent");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("nonexistent.toml"));
    }

    #[test]
    fn current_step_finds_by_id() {
        let dir = tempdir().unwrap();
        write_playbook(dir.path(), "sample", SAMPLE_TOML);
        let pb = load_playbook(dir.path(), "sample").unwrap();

        let step = current_step(&pb, "beta").unwrap();
        assert_eq!(step.id, "beta");
        assert_eq!(step.required_role, "Developer");
    }

    #[test]
    fn current_step_missing_id_returns_none() {
        let dir = tempdir().unwrap();
        write_playbook(dir.path(), "sample", SAMPLE_TOML);
        let pb = load_playbook(dir.path(), "sample").unwrap();

        assert!(current_step(&pb, "does_not_exist").is_none());
    }

    #[test]
    fn next_step_follows_next_field() {
        let dir = tempdir().unwrap();
        write_playbook(dir.path(), "sample", SAMPLE_TOML);
        let pb = load_playbook(dir.path(), "sample").unwrap();

        let step = next_step(&pb, "alpha").unwrap();
        assert_eq!(step.id, "beta");

        let step2 = next_step(&pb, "beta").unwrap();
        assert_eq!(step2.id, "gamma");
    }

    #[test]
    fn next_step_at_last_step_returns_none() {
        let dir = tempdir().unwrap();
        write_playbook(dir.path(), "sample", SAMPLE_TOML);
        let pb = load_playbook(dir.path(), "sample").unwrap();

        assert!(next_step(&pb, "gamma").is_none());
    }

    #[test]
    fn required_role_returns_role_string() {
        let dir = tempdir().unwrap();
        write_playbook(dir.path(), "sample", SAMPLE_TOML);
        let pb = load_playbook(dir.path(), "sample").unwrap();

        assert_eq!(required_role(&pb, "alpha").as_deref(), Some("Architect"));
        assert_eq!(required_role(&pb, "gamma").as_deref(), Some("Conductor"));
        assert!(required_role(&pb, "missing").is_none());
    }

    #[test]
    fn load_all_real_playbooks() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let playbooks_dir = Path::new(&manifest_dir).join("playbooks");

        let expected = ["bugfix", "documentation", "feature", "investigation", "refactor"];
        for name in &expected {
            let pb = load_playbook(&playbooks_dir, name)
                .unwrap_or_else(|e| panic!("failed to load playbook '{}': {}", name, e));
            assert!(!pb.name.is_empty(), "playbook '{}' has empty name", name);
            assert!(!pb.description.is_empty(), "playbook '{}' has empty description", name);
            assert!(!pb.steps.is_empty(), "playbook '{}' has no steps", name);
            for step in &pb.steps {
                assert!(!step.id.is_empty(), "step in '{}' has empty id", name);
                assert!(!step.description.is_empty(), "step '{}' in '{}' has empty description", step.id, name);
                assert!(!step.required_role.is_empty(), "step '{}' in '{}' has empty required_role", step.id, name);
            }
            // Last step must have no next
            assert!(
                pb.steps.last().unwrap().next.is_none(),
                "last step of '{}' should have no next",
                name
            );
        }
    }
}
