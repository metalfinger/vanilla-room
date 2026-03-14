use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::types::{DiscussionEntry, Vote};

// Maximum content length before truncation in format_for_prompt
const MAX_CONTENT_LEN: usize = 4000;

// ---------------------------------------------------------------------------
// Discussion
// ---------------------------------------------------------------------------

/// Append-only JSONL discussion log — the bus for all agent communication.
pub struct Discussion {
    path: PathBuf,
}

impl Discussion {
    /// Create or open a discussion log at the given path.
    pub fn new(path: PathBuf) -> io::Result<Self> {
        // Touch the file so it exists; do nothing if it already does.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self { path })
    }

    /// Serialize `entry` to JSON and append it as a single line to the file.
    pub fn append(&self, entry: &DiscussionEntry) -> io::Result<()> {
        let line = serde_json::to_string(entry)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let lock_path = self.path.with_extension("jsonl.lock");

        // Simple lock file with retry
        let mut acquired = false;
        for _ in 0..20 {
            match OpenOptions::new().write(true).create_new(true).open(&lock_path) {
                Ok(_) => { acquired = true; break; }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }

        // Write the entry (with or without lock)
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", line)?;

        // Release lock
        if acquired {
            let _ = std::fs::remove_file(&lock_path);
        }

        Ok(())
    }

    /// Read all entries from the log.
    pub fn read_all(&self) -> io::Result<Vec<DiscussionEntry>> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let reader = BufReader::new(file);
        let mut entries = Vec::new();
        for (lineno, line) in reader.lines().enumerate() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<DiscussionEntry>(trimmed) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    eprintln!("Warning: skipping corrupt discussion line {}: {}", lineno + 1, e);
                    continue;
                }
            }
        }
        Ok(entries)
    }

    /// Read the last `n` entries from the log efficiently.
    ///
    /// For large files this still reads all lines, but correctness is
    /// guaranteed without seeking backwards through a byte stream.
    pub fn read_last(&self, n: usize) -> io::Result<Vec<DiscussionEntry>> {
        if n == 0 {
            return Ok(vec![]);
        }
        let all = self.read_all()?;
        let start = all.len().saturating_sub(n);
        Ok(all[start..].to_vec())
    }

    /// Format a slice of entries as a human-readable transcript for inclusion
    /// in an LLM prompt.
    ///
    /// Each line takes the form:
    /// ```text
    /// [Turn N] AgentName (Role): Content… [STATUS: Vote]
    /// ```
    /// Long content is truncated to `MAX_CONTENT_LEN` characters.
    pub fn format_for_prompt(entries: &[DiscussionEntry]) -> String {
        let mut lines = Vec::with_capacity(entries.len());
        for (i, entry) in entries.iter().enumerate() {
            let turn = i + 1;
            let content = truncate(&entry.content, MAX_CONTENT_LEN);
            let status_suffix = match &entry.status_vote {
                Some(vote) => format!(" [STATUS: {}]", vote_label(vote)),
                None => String::new(),
            };
            lines.push(format!(
                "[Turn {}] {} ({}): {}{}",
                turn, entry.agent_name, entry.role, content, status_suffix
            ));
        }
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_owned()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{}…", truncated)
    }
}

fn vote_label(vote: &Vote) -> &'static str {
    match vote {
        Vote::Approved => "APPROVED",
        Vote::Rejected => "REJECTED",
        Vote::Discussing => "DISCUSSING",
        Vote::Blocking => "BLOCKING",
        Vote::Pending => "PENDING",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn make_entry(name: &str, role: &str, content: &str, vote: Option<Vote>) -> DiscussionEntry {
        DiscussionEntry {
            timestamp: Utc::now(),
            agent_name: name.to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            status_vote: vote,
            handoff_targets: vec![],
            decisions: vec![],
            artifacts: vec![],
        }
    }

    fn temp_path() -> PathBuf {
        NamedTempFile::new().unwrap().into_temp_path().to_path_buf()
    }

    #[test]
    fn test_round_trip() {
        let path = temp_path();
        let disc = Discussion::new(path.clone()).unwrap();

        let e1 = make_entry("Conductor", "orchestrator", "Objective: Add JWT auth", None);
        let e2 = make_entry("Architect", "designer", "I propose...", Some(Vote::Discussing));
        disc.append(&e1).unwrap();
        disc.append(&e2).unwrap();

        let all = disc.read_all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].agent_name, "Conductor");
        assert_eq!(all[1].status_vote, Some(Vote::Discussing));
    }

    #[test]
    fn test_read_last() {
        let path = temp_path();
        let disc = Discussion::new(path).unwrap();
        for i in 0..5 {
            let e = make_entry(&format!("Agent{}", i), "role", "msg", None);
            disc.append(&e).unwrap();
        }
        let last2 = disc.read_last(2).unwrap();
        assert_eq!(last2.len(), 2);
        assert_eq!(last2[0].agent_name, "Agent3");
        assert_eq!(last2[1].agent_name, "Agent4");
    }

    #[test]
    fn test_format_for_prompt() {
        let entries = vec![
            make_entry("Conductor", "orchestrator", "Objective: Add JWT auth", None),
            make_entry("Architect", "designer", "I propose...", Some(Vote::Discussing)),
            make_entry("Security", "reviewer", "Concern about...", Some(Vote::Discussing)),
        ];
        let prompt = Discussion::format_for_prompt(&entries);
        assert!(prompt.contains("[Turn 1] Conductor (orchestrator): Objective: Add JWT auth"));
        assert!(prompt.contains("[Turn 2] Architect (designer): I propose... [STATUS: DISCUSSING]"));
        assert!(prompt.contains("[Turn 3] Security (reviewer): Concern about... [STATUS: DISCUSSING]"));
    }

    #[test]
    fn test_truncation() {
        let long_content = "a".repeat(5000);
        let entry = make_entry("Agent", "role", &long_content, None);
        let prompt = Discussion::format_for_prompt(&[entry]);
        // Should contain the ellipsis character and not the full 5000 chars of 'a'
        assert!(prompt.contains('…'));
        assert!(!prompt.contains(&"a".repeat(5000)));
        // But should preserve up to 4000 chars
        assert!(prompt.contains(&"a".repeat(4000)));
    }
}
