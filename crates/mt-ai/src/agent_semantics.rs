//! Narrow native terminal decoding. This does not establish liveness or ownership;
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

/// A bounded row from the active VT grid, never from the scrolled viewport.
pub struct AgentScreenRow<'a> {
    pub text: &'a str,
    pub first_cell_bold: bool,
    pub wrapped: bool,
}

/// Keep the native counter in the evidence: output or identical repaint is not
/// a new observation. Composer drafts and cursor movement are not evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexScreenEvidence {
    pub activity: AgentActivity,
    pub marker: String,
    pub marker_row: usize,
    pub footer: String,
}

impl CodexScreenEvidence {
    pub fn same_status(&self, other: &Self) -> bool {
        self.activity == other.activity && self.marker == other.marker
    }
}

pub fn codex_screen_evidence(
    rows: &[AgentScreenRow<'_>],
    cursor_row: usize,
    cursor_visible: bool,
) -> Option<CodexScreenEvidence> {
    if !cursor_visible
        || rows.len() > 25
        || cursor_row >= rows.len()
        || rows
            .iter()
            .any(|row| row.text.len() > 2048 || row.text.chars().any(char::is_control))
    {
        return None;
    }
    let mut composer = cursor_row;
    loop {
        let row = &rows[composer];
        let text = row.text.trim_start();
        if row.first_cell_bold && (text.starts_with("\u{203a} ") || text.starts_with("\u{00bb} ")) {
            break;
        }
        if composer == 0
            || cursor_row - composer >= 6
            || text.is_empty()
            || !row.text.starts_with("  ")
            || text.starts_with(['\u{203a}', '\u{00bb}'])
        {
            return None;
        }
        composer -= 1;
    }
    // A gap and a model/cwd footer distinguish the current composer from quoted
    // transcript prompts. Overlays and truncated/wrapped status rows fail closed.
    let footer_index =
        (cursor_row + 1..rows.len()).find(|index| !rows[*index].text.trim().is_empty())?;
    if footer_index <= cursor_row + 1
        || footer_index > cursor_row + 4
        || rows[footer_index].wrapped
        || rows[footer_index + 1..]
            .iter()
            .any(|row| !row.text.trim().is_empty())
    {
        return None;
    }
    let footer = rows[footer_index].text.trim();
    if !codex_model_cwd_footer(footer) {
        return None;
    }
    let status_index = (composer.saturating_sub(3)..composer)
        .rev()
        .find(|index| !rows[*index].text.trim().is_empty())?;
    let row = &rows[status_index];
    if row.wrapped || rows[composer..=cursor_row].iter().any(|row| row.wrapped) {
        return None;
    }
    let status = codex_screen_marker(row.text)?;
    Some(CodexScreenEvidence {
        activity: AgentActivity::Working,
        marker: status.to_string(),
        marker_row: status_index,
        footer: footer.to_string(),
    })
}

/// Text-only marker comparison is a non-renewal fence, not semantic evidence.
/// Positive interpretation still requires the complete Codex composer context.
pub fn codex_screen_marker(text: &str) -> Option<&str> {
    if text.len() > 2048 || text.chars().any(char::is_control) {
        return None;
    }
    let marker = text.trim();
    let status = marker
        .strip_prefix("\u{2022} ")
        .or_else(|| marker.strip_prefix("\u{25e6} "))
        .unwrap_or(marker);
    let elapsed = status
        .strip_prefix("Working (")?
        .strip_suffix(" \u{2022} esc to interrupt)")?;
    if !codex_elapsed(elapsed) {
        return None;
    }
    Some(status)
}

fn codex_model_cwd_footer(footer: &str) -> bool {
    let mut parts = footer.split(['\u{00b7}', '\u{2022}']).map(str::trim);
    let Some(model_part) = parts.next() else {
        return false;
    };
    let mut words = model_part.split_whitespace();
    let Some(model) = words.next() else {
        return false;
    };
    if !model
        .bytes()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'.' | b'_'))
        || words.next().is_some_and(|reasoning| {
            !matches!(
                reasoning,
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            )
        })
        || words.next().is_some()
    {
        return false;
    }
    let known_model = model
        .strip_prefix("gpt-")
        .is_some_and(|tail| !tail.is_empty())
        || model
            .strip_prefix('o')
            .is_some_and(|tail| tail.starts_with(|ch: char| ch.is_ascii_digit()));
    known_model
        && parts.any(|part| {
            part.starts_with('/')
                || part.starts_with("~/")
                || (part.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                    && part
                        .get(1..3)
                        .is_some_and(|prefix| prefix == ":\\" || prefix == ":/"))
        })
}

fn codex_elapsed(elapsed: &str) -> bool {
    if elapsed.len() > 24 {
        return false;
    }
    let parts = elapsed.split(' ').collect::<Vec<_>>();
    let units: &[char] = match parts.len() {
        1 => &['s'],
        2 => &['m', 's'],
        3 => &['h', 'm', 's'],
        _ => return false,
    };
    parts
        .iter()
        .zip(units)
        .enumerate()
        .all(|(index, (part, unit))| {
            part.strip_suffix(*unit).is_some_and(|number| {
                !number.is_empty()
                    && number.bytes().all(|byte| byte.is_ascii_digit())
                    && (index == 0 || number.len() == 2)
                    && number.parse::<u32>().is_ok_and(|value| {
                        if index == 0 && parts.len() > 1 {
                            value > 0 && (parts.len() == 3 || value < 60)
                        } else {
                            value < 60
                        }
                    })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen<'a>(lines: &'a [&'a str]) -> Vec<AgentScreenRow<'a>> {
        lines
            .iter()
            .map(|text| AgentScreenRow {
                text,
                first_cell_bold: text.trim_start().starts_with('\u{203a}'),
                wrapped: false,
            })
            .collect()
    }

    #[test]
    fn codex_screen_working_requires_native_status_composer_and_footer() {
        for status in [
            "Working (2s \u{2022} esc to interrupt)",
            "\u{2022} Working (1m 02s \u{2022} esc to interrupt)",
            "\u{25e6} Working (1h 02m 03s \u{2022} esc to interrupt)",
        ] {
            for prompt in ["\u{203a} Ask Codex to do anything", "\u{203a} next draft"] {
                let lines = [
                    status,
                    "",
                    prompt,
                    "",
                    "gpt-6-astra max \u{00b7} ~/mini-term",
                    "",
                ];
                let evidence = codex_screen_evidence(&screen(&lines), 2, true).unwrap();
                assert_eq!(evidence.activity, AgentActivity::Working);
                assert_eq!(
                    evidence.marker,
                    status.trim_start_matches(['\u{2022}', '\u{25e6}', ' '])
                );
            }
        }
    }

    #[test]
    fn codex_screen_idle_and_transcript_words_never_invent_task_state() {
        for status in [
            "",
            "Working",
            "done",
            "Waiting for input",
            "Please approve",
            "quoted: Working (2s \u{2022} esc to interrupt)",
            "Working (2s \u{2022} esc to interrupt) is a status example",
            "Working (2s \u{2022} esc to interrupt",
            "Working (forever \u{2022} esc to interrupt)",
            "Working (1m 99s \u{2022} esc to interrupt)",
            "Working (99s \u{2022} esc to interrupt)",
        ] {
            let lines = [
                status,
                "",
                "\u{203a} Ask Codex to do anything",
                "",
                "gpt-5.4 \u{00b7} /repo",
            ];
            assert!(codex_screen_evidence(&screen(&lines), 2, true).is_none());
        }
    }

    #[test]
    fn codex_screen_rejects_unframed_hidden_wrapped_or_incomplete_context() {
        let lines = [
            "Working (2s \u{2022} esc to interrupt)",
            "",
            "\u{203a} draft",
            "",
            "gpt-5.4 \u{00b7} /repo",
        ];
        let mut rows = screen(&lines);
        assert!(codex_screen_evidence(&rows, 2, false).is_none());
        rows[2].first_cell_bold = false;
        assert!(codex_screen_evidence(&rows, 2, true).is_none());
        rows[2].first_cell_bold = true;
        rows[0].wrapped = true;
        assert!(codex_screen_evidence(&rows, 2, true).is_none());
        rows[0].wrapped = false;
        rows[4].text = "ordinary footer";
        assert!(codex_screen_evidence(&rows, 2, true).is_none());
        rows[4].text = "gpt-5.4 \u{00b7} /repo\x1b";
        assert!(codex_screen_evidence(&rows, 2, true).is_none());
        assert!(codex_screen_evidence(&rows[..4], 2, true).is_none());
    }

    #[test]
    fn native_title_markers_are_provider_specific() {
        for (provider, title, activity) in [
            ("pi", "\u{03c0} : project", AgentActivity::Working),
            ("pi", "\u{03c0} ! approve tool", AgentActivity::Blocked),
            (
                "pi",
                "\u{03c0} > waiting label : working",
                AgentActivity::Waiting,
            ),
            (
                "opencode",
                "\u{280b} OC | conversation",
                AgentActivity::Working,
            ),
            ("opencode", "OC | conversation", AgentActivity::Waiting),
            ("claude", "\u{2733} task title", AgentActivity::Waiting),
            ("claude", "\u{280b} Claude Code", AgentActivity::Working),
            (
                "grok",
                "\u{280b} - reading files - grok",
                AgentActivity::Working,
            ),
        ] {
            assert_eq!(
                activity_from_owned_title(&provider.parse().unwrap(), title),
                Some(activity)
            );
            assert_eq!(
                activity_from_owned_title(&"codex".parse().unwrap(), title),
                None
            );
        }
    }

    #[test]
    fn arbitrary_titles_redraws_and_transcripts_have_no_semantics() {
        for provider in ["claude", "codex", "opencode", "pi", "grok"] {
            for title in [
                "",
                "working",
                "done",
                "permission required",
                "codex - working",
                "shell | pi : project",
                "\u{280b} arbitrary task",
                "\x1b[2J",
                "\u{03c0}: project",
                "OC | ",
                "\u{280b} fix tests - grok",
            ] {
                assert_eq!(
                    activity_from_owned_title(&provider.parse().unwrap(), title),
                    None
                );
            }
        }
        assert_eq!(
            activity_from_owned_title(&"pi".parse().unwrap(), &"x".repeat(1025)),
            None
        );
    }
}
