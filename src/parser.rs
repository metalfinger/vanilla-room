use crate::types::{ParsedResponse, Vote};

/// Remove fenced code blocks (``` ... ```) and inline code (` ... `) from `raw`,
/// replacing them with empty strings so that tags inside code are not parsed.
fn strip_code_blocks(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut i = 0;
    let bytes = raw.as_bytes();
    let len = bytes.len();

    while i < len {
        // Check for triple backtick fence
        if bytes[i] == b'`'
            && i + 2 < len
            && bytes[i + 1] == b'`'
            && bytes[i + 2] == b'`'
        {
            // Skip opening fence (```) and everything until closing ```
            i += 3;
            loop {
                if i + 2 < len
                    && bytes[i] == b'`'
                    && bytes[i + 1] == b'`'
                    && bytes[i + 2] == b'`'
                {
                    i += 3;
                    break;
                }
                if i >= len {
                    break;
                }
                i += 1;
            }
        } else if bytes[i] == b'`' {
            // Inline code: skip until the matching closing backtick
            i += 1;
            while i < len && bytes[i] != b'`' {
                i += 1;
            }
            if i < len {
                i += 1; // consume closing backtick
            }
        } else {
            // Safe to push: advance by one UTF-8 char, not one byte
            let ch = raw[i..].chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
        }
    }

    result
}

/// Extract all `[TAG: value]` occurrences from `raw` where `tag` matches (case-insensitive).
/// Returns an iterator of trimmed value strings.
fn extract_tag_values<'a>(raw: &'a str, tag: &str) -> Vec<String> {
    let tag_upper = tag.to_uppercase();
    let mut results = Vec::new();
    let mut haystack = raw;

    while let Some(open) = haystack.find('[') {
        let rest = &haystack[open + 1..];
        if let Some(close) = rest.find(']') {
            let inner = &rest[..close];
            if let Some(colon) = inner.find(':') {
                let found_tag = inner[..colon].trim().to_uppercase();
                if found_tag == tag_upper {
                    let value = inner[colon + 1..].trim().to_string();
                    results.push(value);
                }
            }
            // Advance past the closing bracket
            haystack = &rest[close + 1..];
        } else {
            // No closing bracket found; stop scanning
            break;
        }
    }

    results
}

/// Parse the STATUS tag value into a `Vote`.
fn parse_vote(s: &str) -> Option<Vote> {
    match s.trim().to_uppercase().as_str() {
        "APPROVED" => Some(Vote::Approved),
        "REJECTED" => Some(Vote::Rejected),
        "DISCUSSING" => Some(Vote::Discussing),
        "BLOCKING" => Some(Vote::Blocking),
        "PENDING" => Some(Vote::Pending),
        _ => None,
    }
}

/// Parse structured tags from an agent natural-language response.
pub fn parse_response(raw: &str) -> ParsedResponse {
    let clean = strip_code_blocks(raw);

    // STATUS — use last occurrence if multiple (most recent wins)
    let status = extract_tag_values(&clean, "STATUS")
        .into_iter()
        .filter_map(|v| parse_vote(&v))
        .last();

    // HANDOFF — split comma-separated targets, trim whitespace
    let handoff_targets = extract_tag_values(&clean, "HANDOFF")
        .into_iter()
        .flat_map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .collect();

    let decisions = extract_tag_values(&clean, "DECISION");
    let artifacts = extract_tag_values(&clean, "ARTIFACT");
    let recruit_requests = extract_tag_values(&clean, "RECRUIT");
    let deboard_requests = extract_tag_values(&clean, "DEBOARD");

    ParsedResponse {
        raw_content: raw.to_string(),
        status,
        handoff_targets,
        decisions,
        artifacts,
        recruit_requests,
        deboard_requests,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_status_approved() {
        let r = parse_response("Looks good to me. [STATUS: APPROVED]");
        assert_eq!(r.status, Some(Vote::Approved));
        assert!(r.handoff_targets.is_empty());
        assert!(r.decisions.is_empty());
    }

    #[test]
    fn single_status_rejected() {
        let r = parse_response("[STATUS: REJECTED] The implementation is wrong.");
        assert_eq!(r.status, Some(Vote::Rejected));
    }

    #[test]
    fn single_status_discussing() {
        let r = parse_response("I have questions. [STATUS: DISCUSSING]");
        assert_eq!(r.status, Some(Vote::Discussing));
    }

    #[test]
    fn single_status_blocking() {
        let r = parse_response("[STATUS: BLOCKING] We cannot proceed.");
        assert_eq!(r.status, Some(Vote::Blocking));
    }

    #[test]
    fn status_case_insensitive() {
        let r = parse_response("[status: approved]");
        assert_eq!(r.status, Some(Vote::Approved));

        let r2 = parse_response("[Status: Rejected]");
        assert_eq!(r2.status, Some(Vote::Rejected));
    }

    #[test]
    fn multiple_status_uses_last() {
        // When the agent updates their vote mid-message, the last one wins
        let r = parse_response("[STATUS: DISCUSSING] ... actually [STATUS: APPROVED]");
        assert_eq!(r.status, Some(Vote::Approved));
    }

    #[test]
    fn multiple_decisions() {
        let raw = "Here are my decisions: \
            [DECISION: Use JWT with 1h expiry] \
            [DECISION: Postgres for persistence] \
            [DECISION: REST over gRPC]";
        let r = parse_response(raw);
        assert_eq!(r.decisions.len(), 3);
        assert_eq!(r.decisions[0], "Use JWT with 1h expiry");
        assert_eq!(r.decisions[1], "Postgres for persistence");
        assert_eq!(r.decisions[2], "REST over gRPC");
    }

    #[test]
    fn handoff_single_target() {
        let r = parse_response("Passing to the next agent. [HANDOFF: Developer]");
        assert_eq!(r.handoff_targets, vec!["Developer"]);
    }

    #[test]
    fn handoff_multiple_targets() {
        let r = parse_response("[HANDOFF: Reviewer, Tester, Deployer]");
        assert_eq!(r.handoff_targets, vec!["Reviewer", "Tester", "Deployer"]);
    }

    #[test]
    fn handoff_multiple_tags_merged() {
        let r = parse_response("[HANDOFF: Reviewer] ... [HANDOFF: Tester]");
        assert_eq!(r.handoff_targets, vec!["Reviewer", "Tester"]);
    }

    #[test]
    fn artifact_extraction() {
        let r = parse_response("I produced [ARTIFACT: design.md] and [ARTIFACT: schema.sql]");
        assert_eq!(r.artifacts, vec!["design.md", "schema.sql"]);
    }

    #[test]
    fn recruit_and_deboard() {
        let raw = "We need more help. [RECRUIT: database_expert] \
            Also dropping the researcher. [DEBOARD: researcher]";
        let r = parse_response(raw);
        assert_eq!(r.recruit_requests, vec!["database_expert"]);
        assert_eq!(r.deboard_requests, vec!["researcher"]);
    }

    #[test]
    fn no_tags_returns_empty() {
        let r = parse_response("Just a plain message with no structured tags at all.");
        assert_eq!(r.status, None);
        assert!(r.handoff_targets.is_empty());
        assert!(r.decisions.is_empty());
        assert!(r.artifacts.is_empty());
        assert!(r.recruit_requests.is_empty());
        assert!(r.deboard_requests.is_empty());
        assert_eq!(r.raw_content, "Just a plain message with no structured tags at all.");
    }

    #[test]
    fn mixed_tags_in_natural_language() {
        let raw = "After careful review I believe this design is solid. \
            [STATUS: APPROVED] \
            The team should hand off to implementation now. \
            [HANDOFF: Developer] \
            We settled on [DECISION: Use JWT with 1h expiry] for auth. \
            Output is in [ARTIFACT: design.md]. \
            We should bring in [RECRUIT: security_expert] and let go of \
            [DEBOARD: brainstormer].";
        let r = parse_response(raw);
        assert_eq!(r.status, Some(Vote::Approved));
        assert_eq!(r.handoff_targets, vec!["Developer"]);
        assert_eq!(r.decisions, vec!["Use JWT with 1h expiry"]);
        assert_eq!(r.artifacts, vec!["design.md"]);
        assert_eq!(r.recruit_requests, vec!["security_expert"]);
        assert_eq!(r.deboard_requests, vec!["brainstormer"]);
    }

    #[test]
    fn raw_content_preserved() {
        let raw = "Hello [STATUS: APPROVED]";
        let r = parse_response(raw);
        assert_eq!(r.raw_content, raw);
    }

    #[test]
    fn tag_value_case_insensitive_vote() {
        let r = parse_response("[STATUS: approved]");
        assert_eq!(r.status, Some(Vote::Approved));
        let r2 = parse_response("[STATUS: Blocking]");
        assert_eq!(r2.status, Some(Vote::Blocking));
    }

    #[test]
    fn unknown_status_value_is_none() {
        let r = parse_response("[STATUS: UNKNOWN_VALUE]");
        assert_eq!(r.status, None);
    }

    #[test]
    fn tags_inside_code_block_ignored() {
        let raw = "Here's an example:\n```\n[STATUS: APPROVED]\n```\nI'm still reviewing. [STATUS: DISCUSSING]";
        let r = parse_response(raw);
        assert_eq!(r.status, Some(Vote::Discussing));
    }

    #[test]
    fn tags_inside_inline_code_ignored() {
        let raw = "Use `[HANDOFF: Developer]` to pass control. [STATUS: APPROVED]";
        let r = parse_response(raw);
        assert!(r.handoff_targets.is_empty());
        assert_eq!(r.status, Some(Vote::Approved));
    }

    #[test]
    fn tags_inside_fenced_with_lang_ignored() {
        let raw = "```json\n{\"status\": \"[STATUS: REJECTED]\"}\n```\n[STATUS: APPROVED]";
        let r = parse_response(raw);
        assert_eq!(r.status, Some(Vote::Approved));
    }
}
