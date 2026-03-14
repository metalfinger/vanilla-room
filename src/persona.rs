use std::path::Path;

use serde::Deserialize;

use crate::types::AgentProfile;

// ---------------------------------------------------------------------------
// Raw TOML intermediate structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawPersona {
    agent: RawAgent,
    tools: RawTools,
}

#[derive(Debug, Deserialize)]
struct RawAgent {
    name: String,
    role: String,
    personality: String,
    capabilities: Vec<String>,
    trigger_keywords: Vec<String>,
    preferred_successors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawTools {
    allowed: Vec<String>,
}

impl From<RawPersona> for AgentProfile {
    fn from(raw: RawPersona) -> Self {
        AgentProfile {
            name: raw.agent.name,
            role: raw.agent.role,
            personality: raw.agent.personality,
            capabilities: raw.agent.capabilities,
            trigger_keywords: raw.agent.trigger_keywords,
            preferred_successors: raw.agent.preferred_successors,
            allowed_tools: raw.tools.allowed,
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum PersonaError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Message(String),
}

impl std::fmt::Display for PersonaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PersonaError::Io(e) => write!(f, "IO error: {}", e),
            PersonaError::Parse(e) => write!(f, "TOML parse error: {}", e),
            PersonaError::Message(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for PersonaError {}

impl From<std::io::Error> for PersonaError {
    fn from(e: std::io::Error) -> Self {
        PersonaError::Io(e)
    }
}

impl From<toml::de::Error> for PersonaError {
    fn from(e: toml::de::Error) -> Self {
        PersonaError::Parse(e)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load a persona by name from `personas_dir/{name}.toml`.
pub fn load_persona(personas_dir: &Path, name: &str) -> Result<AgentProfile, PersonaError> {
    let path = personas_dir.join(format!("{}.toml", name));
    let contents = std::fs::read_to_string(&path).map_err(|e| {
        PersonaError::Message(format!(
            "failed to read persona file '{}': {}",
            path.display(),
            e
        ))
    })?;
    let raw: RawPersona = toml::from_str(&contents).map_err(|e| {
        PersonaError::Message(format!(
            "failed to parse persona file '{}': {}",
            path.display(),
            e
        ))
    })?;
    Ok(AgentProfile::from(raw))
}

/// List all persona names available in `personas_dir` (filenames without `.toml`).
pub fn list_available(personas_dir: &Path) -> Result<Vec<String>, PersonaError> {
    let mut names: Vec<String> = std::fs::read_dir(personas_dir)
        .map_err(|e| {
            PersonaError::Message(format!(
                "failed to read personas directory '{}': {}",
                personas_dir.display(),
                e
            ))
        })?
        .filter_map(|entry: std::io::Result<std::fs::DirEntry>| {
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
    Ok(names)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_persona(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(format!("{}.toml", name)), content).unwrap();
    }

    const SAMPLE_TOML: &str = r#"
[agent]
name = "Developer"
role = "Senior Developer"
personality = "Writes production-ready code."
capabilities = ["implementation", "testing"]
trigger_keywords = ["implement", "code"]
preferred_successors = ["Tester", "Reviewer"]

[tools]
allowed = ["Edit", "Write", "Read", "Bash", "Glob", "Grep"]
"#;

    #[test]
    fn load_persona_parses_fields() {
        let dir = tempdir().unwrap();
        write_persona(dir.path(), "developer", SAMPLE_TOML);

        let profile = load_persona(dir.path(), "developer").unwrap();
        assert_eq!(profile.name, "Developer");
        assert_eq!(profile.role, "Senior Developer");
        assert_eq!(profile.capabilities, vec!["implementation", "testing"]);
        assert_eq!(profile.trigger_keywords, vec!["implement", "code"]);
        assert_eq!(profile.preferred_successors, vec!["Tester", "Reviewer"]);
        assert_eq!(
            profile.allowed_tools,
            vec!["Edit", "Write", "Read", "Bash", "Glob", "Grep"]
        );
    }

    #[test]
    fn load_persona_missing_file_returns_error() {
        let dir = tempdir().unwrap();
        let result = load_persona(dir.path(), "nonexistent");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("nonexistent.toml"));
    }

    #[test]
    fn list_available_returns_sorted_names() {
        let dir = tempdir().unwrap();
        write_persona(dir.path(), "tester", SAMPLE_TOML);
        write_persona(dir.path(), "architect", SAMPLE_TOML);
        write_persona(dir.path(), "developer", SAMPLE_TOML);
        // A non-toml file that should be ignored
        fs::write(dir.path().join("notes.txt"), "ignore me").unwrap();

        let names = list_available(dir.path()).unwrap();
        assert_eq!(names, vec!["architect", "developer", "tester"]);
    }

    #[test]
    fn list_available_empty_dir_returns_empty_vec() {
        let dir = tempdir().unwrap();
        let names = list_available(dir.path()).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn list_available_missing_dir_returns_error() {
        let result = list_available(Path::new("/nonexistent/path/personas"));
        assert!(result.is_err());
    }

    #[test]
    fn load_all_real_personas() {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let personas_dir = Path::new(&manifest_dir).join("personas");

        let names = list_available(&personas_dir).unwrap();
        assert_eq!(
            names.len(),
            10,
            "expected 10 persona files, got {}: {:?}",
            names.len(),
            names
        );

        for name in &names {
            let profile = load_persona(&personas_dir, name)
                .unwrap_or_else(|e| panic!("failed to load persona '{}': {}", name, e));
            assert!(!profile.name.is_empty(), "persona '{}' has empty name", name);
            assert!(!profile.role.is_empty(), "persona '{}' has empty role", name);
            assert!(
                !profile.personality.is_empty(),
                "persona '{}' has empty personality",
                name
            );
            assert!(
                !profile.capabilities.is_empty(),
                "persona '{}' has no capabilities",
                name
            );
            assert!(
                !profile.allowed_tools.is_empty(),
                "persona '{}' has no allowed tools",
                name
            );
        }
    }
}
