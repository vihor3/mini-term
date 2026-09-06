//! Narrow native title decoding. This does not establish liveness or ownership;
//! callers must submit the result through the exact-run semantic boundary.

use crate::{AgentActivity, AgentProvider};

pub fn activity_from_owned_title(provider: &AgentProvider, title: &str) -> Option<AgentActivity> {
    if title.len() > 1024 || title.chars().any(char::is_control) {
        return None;
    }
    let title = title.trim();
    let (spinner, body) = leading_spinner(title);
    match provider.as_str() {
        AgentProvider::PI => {
            let mut words = body.split_whitespace();
            if words.next()? != "\u{03c0}" {
                return None;
            }
            match words.next()? {
                ":" => Some(AgentActivity::Working),
                "!" => Some(AgentActivity::Blocked),
                ">" => Some(AgentActivity::Waiting),
                "-" if spinner => Some(AgentActivity::Working),
                _ => None,
            }
        }
        AgentProvider::OPENCODE => {
            let body = body.strip_prefix("\u{25a3} ").unwrap_or(body);
            let label = body.strip_prefix("OC | ")?;
            (!label.trim().is_empty()).then_some(if spinner {
                AgentActivity::Working
            } else {
                AgentActivity::Waiting
            })
        }
        AgentProvider::CLAUDE => {
            if title == "\u{2733}" || title.starts_with("\u{2733} ") {
                Some(AgentActivity::Waiting)
            } else if spinner && (body == "Claude" || body.starts_with("Claude ")) {
                Some(AgentActivity::Working)
            } else {
                None
            }
        }
        AgentProvider::GROK if spinner => {
            let body = body.to_ascii_lowercase();
            (body == "grok" || (body.starts_with("- ") && body.ends_with(" - grok")))
                .then_some(AgentActivity::Working)
        }
        // Codex conversation names and ordinary task text carry no task state.
        _ => None,
    }
}

fn leading_spinner(title: &str) -> (bool, &str) {
    let body = title.trim_start_matches(|ch| ('\u{2800}'..='\u{28ff}').contains(&ch));
    if body.len() < title.len() && body.starts_with(char::is_whitespace) {
        (true, body.trim_start())
    } else {
        (false, title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_title_markers_are_provider_specific() {
        for (provider, title, activity) in [
            ("pi", "\u{03c0} : project", AgentActivity::Working),
            ("pi", "\u{03c0} ! approve tool", AgentActivity::Blocked),
            ("pi", "\u{03c0} > waiting label : working", AgentActivity::Waiting),
            ("opencode", "\u{280b} OC | conversation", AgentActivity::Working),
            ("opencode", "OC | conversation", AgentActivity::Waiting),
            ("claude", "\u{2733} task title", AgentActivity::Waiting),
            ("claude", "\u{280b} Claude Code", AgentActivity::Working),
            ("grok", "\u{280b} - reading files - grok", AgentActivity::Working),
        ] {
            assert_eq!(activity_from_owned_title(&provider.parse().unwrap(), title), Some(activity));
            assert_eq!(activity_from_owned_title(&"codex".parse().unwrap(), title), None);
        }
    }

    #[test]
    fn arbitrary_titles_redraws_and_transcripts_have_no_semantics() {
        for provider in ["claude", "codex", "opencode", "pi", "grok"] {
            for title in [
                "", "working", "done", "permission required", "codex - working",
                "shell | pi : project", "\u{280b} arbitrary task", "\x1b[2J",
                "\u{03c0}: project", "OC | ", "\u{280b} fix tests - grok",
            ] {
                assert_eq!(activity_from_owned_title(&provider.parse().unwrap(), title), None);
            }
        }
        assert_eq!(activity_from_owned_title(&"pi".parse().unwrap(), &"x".repeat(1025)), None);
    }
}
