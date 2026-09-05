use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};

use super::{GitAnnotation, WorktreeFact, WorktreePathSemantics, WorktreePathState, paths_equal};

#[derive(Default)]
struct RecordBuilder {
    path: Option<PathBuf>,
    head: Option<String>,
    branch_ref: Option<String>,
    detached: Option<bool>,
    bare: Option<bool>,
    sparse: Option<bool>,
    locked: Option<GitAnnotation>,
    prunable: Option<GitAnnotation>,
    saw_field: bool,
}

impl RecordBuilder {
    fn set_value<T: PartialEq>(slot: &mut Option<T>, value: T, field: &str) -> Result<()> {
        match slot {
            Some(existing) if existing != &value => bail!("conflicting duplicate {field} field"),
            Some(_) => Ok(()),
            None => {
                *slot = Some(value);
                Ok(())
            }
        }
    }

    fn set_marker(slot: &mut Option<bool>, field: &str) -> Result<()> {
        Self::set_value(slot, true, field)
    }

    fn finish(self, is_main: bool) -> Result<WorktreeFact> {
        let path = self
            .path
            .ok_or_else(|| anyhow!("record is missing worktree path"))?;
        if path.as_os_str().is_empty() {
            bail!("worktree path must not be empty");
        }
        if self.detached == Some(true) && self.branch_ref.is_some() {
            bail!("record cannot be both detached and attached to a branch");
        }
        Ok(WorktreeFact {
            path,
            head: self.head,
            branch_ref: self.branch_ref,
            is_main,
            is_detached: self.detached.unwrap_or(false),
            is_bare: self.bare.unwrap_or(false),
            is_sparse: self.sparse.unwrap_or(false),
            locked: self.locked,
            prunable: self.prunable,
            path_state: WorktreePathState::Unknown,
        })
    }
}

pub(super) fn parse_porcelain_z(input: &[u8]) -> Result<Vec<WorktreeFact>> {
    parse_porcelain_z_with_path_semantics(input, WorktreePathSemantics::Native)
}

pub(super) fn parse_porcelain_z_with_path_semantics(
    input: &[u8],
    path_semantics: WorktreePathSemantics,
) -> Result<Vec<WorktreeFact>> {
    let fields: Vec<&[u8]> = input.split(|byte| *byte == 0).collect();
    let mut records = Vec::new();
    let mut builder = RecordBuilder::default();

    for (index, field) in fields.iter().enumerate() {
        if field.is_empty() {
            if builder.saw_field {
                records.push(builder.finish(records.is_empty())?);
                builder = RecordBuilder::default();
            } else if fields[index + 1..]
                .iter()
                .any(|remaining| !remaining.is_empty())
            {
                bail!("empty worktree record");
            }
            continue;
        }
        parse_field(field, false, &mut builder)?;
    }

    if builder.saw_field {
        records.push(builder.finish(records.is_empty())?);
    }
    if records.is_empty() {
        bail!("porcelain output contained no worktree records");
    }
    validate_records(&records, path_semantics)?;
    Ok(records)
}

pub(super) fn parse_porcelain_text(input: &[u8]) -> Result<Vec<WorktreeFact>> {
    parse_porcelain_text_with_path_semantics(input, WorktreePathSemantics::Native)
}

pub(super) fn parse_porcelain_text_with_path_semantics(
    input: &[u8],
    path_semantics: WorktreePathSemantics,
) -> Result<Vec<WorktreeFact>> {
    let mut records = Vec::new();
    let mut builder = RecordBuilder::default();

    for raw_line in input.split(|byte| *byte == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if line.is_empty() {
            if builder.saw_field {
                records.push(builder.finish(records.is_empty())?);
                builder = RecordBuilder::default();
            }
            continue;
        }
        parse_field(line, true, &mut builder)?;
    }

    if builder.saw_field {
        records.push(builder.finish(records.is_empty())?);
    }
    if records.is_empty() {
        bail!("porcelain output contained no worktree records");
    }
    validate_records(&records, path_semantics)?;
    Ok(records)
}

fn validate_records(records: &[WorktreeFact], path_semantics: WorktreePathSemantics) -> Result<()> {
    for (index, record) in records.iter().enumerate() {
        if records[..index]
            .iter()
            .any(|previous| paths_equal_for(previous, record, path_semantics))
        {
            bail!("duplicate worktree path in porcelain output");
        }
    }
    Ok(())
}

fn paths_equal_for(
    left: &WorktreeFact,
    right: &WorktreeFact,
    path_semantics: WorktreePathSemantics,
) -> bool {
    match path_semantics {
        WorktreePathSemantics::Native => paths_equal(&left.path, &right.path),
        WorktreePathSemantics::Posix => {
            normalize_posix_path(&left.path.to_string_lossy())
                == normalize_posix_path(&right.path.to_string_lossy())
        }
    }
}

fn normalize_posix_path(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() && path.starts_with('/') {
        "/"
    } else {
        trimmed
    }
}

fn parse_field(field: &[u8], quoted: bool, builder: &mut RecordBuilder) -> Result<()> {
    if builder.path.is_none() && !field.starts_with(b"worktree ") {
        bail!("worktree path must be the first field in a record");
    }
    builder.saw_field = true;
    if let Some(value) = field.strip_prefix(b"worktree ") {
        let value = decode_value(value, quoted)?;
        RecordBuilder::set_value(&mut builder.path, PathBuf::from(value), "worktree")?;
    } else if let Some(value) = field.strip_prefix(b"HEAD ") {
        let value = decode_utf8(value, "HEAD")?;
        if value.is_empty() {
            bail!("HEAD field must not be empty");
        }
        RecordBuilder::set_value(&mut builder.head, value, "HEAD")?;
    } else if let Some(value) = field.strip_prefix(b"branch ") {
        let value = decode_utf8(value, "branch")?;
        if value.is_empty() {
            bail!("branch field must not be empty");
        }
        RecordBuilder::set_value(&mut builder.branch_ref, value, "branch")?;
    } else if field == b"detached" {
        RecordBuilder::set_marker(&mut builder.detached, "detached")?;
    } else if field == b"bare" {
        RecordBuilder::set_marker(&mut builder.bare, "bare")?;
    } else if field == b"sparse" {
        RecordBuilder::set_marker(&mut builder.sparse, "sparse")?;
    } else if field == b"locked" {
        RecordBuilder::set_value(
            &mut builder.locked,
            GitAnnotation { reason: None },
            "locked",
        )?;
    } else if let Some(value) = field.strip_prefix(b"locked ") {
        let reason = decode_value(value, quoted)?;
        if reason.is_empty() {
            bail!("locked reason must not be empty");
        }
        RecordBuilder::set_value(
            &mut builder.locked,
            GitAnnotation {
                reason: Some(reason),
            },
            "locked",
        )?;
    } else if field == b"prunable" {
        RecordBuilder::set_value(
            &mut builder.prunable,
            GitAnnotation { reason: None },
            "prunable",
        )?;
    } else if let Some(value) = field.strip_prefix(b"prunable ") {
        let reason = decode_value(value, quoted)?;
        if reason.is_empty() {
            bail!("prunable reason must not be empty");
        }
        RecordBuilder::set_value(
            &mut builder.prunable,
            GitAnnotation {
                reason: Some(reason),
            },
            "prunable",
        )?;
    } else {
        if field == b"worktree" || field == b"HEAD" || field == b"branch" {
            bail!("porcelain field is missing its value");
        }
        decode_utf8(field, "unknown porcelain")?;
    }
    Ok(())
}

fn decode_utf8(value: &[u8], field: &str) -> Result<String> {
    let decoded = std::str::from_utf8(value)
        .map(str::to_owned)
        .map_err(|_| anyhow!("{field} field is not valid UTF-8"))?;
    if decoded.contains('\0') {
        bail!("{field} field contains NUL");
    }
    Ok(decoded)
}

fn decode_value(value: &[u8], quoted: bool) -> Result<String> {
    if quoted && value.first() == Some(&b'"') {
        decode_c_quoted(value)
    } else {
        decode_utf8(value, "porcelain value")
    }
}

fn decode_c_quoted(value: &[u8]) -> Result<String> {
    if value.len() < 2 || value[0] != b'"' || *value.last().unwrap_or(&0) != b'"' {
        bail!("malformed Git C-quoted value");
    }
    let mut decoded = Vec::with_capacity(value.len() - 2);
    let mut index = 1;
    while index < value.len() - 1 {
        let byte = value[index];
        if byte != b'\\' {
            if byte == b'"' {
                bail!("unescaped quote in Git C-quoted value");
            }
            decoded.push(byte);
            index += 1;
            continue;
        }
        index += 1;
        if index >= value.len() - 1 {
            bail!("trailing escape in Git C-quoted value");
        }
        let escaped = value[index];
        match escaped {
            b'"' | b'\\' => decoded.push(escaped),
            b'a' => decoded.push(0x07),
            b'b' => decoded.push(0x08),
            b't' => decoded.push(b'\t'),
            b'n' => decoded.push(b'\n'),
            b'v' => decoded.push(0x0b),
            b'f' => decoded.push(0x0c),
            b'r' => decoded.push(b'\r'),
            b'0'..=b'7' => {
                let mut octal = (escaped - b'0') as u16;
                let mut digits = 1;
                while digits < 3
                    && index + 1 < value.len() - 1
                    && matches!(value[index + 1], b'0'..=b'7')
                {
                    index += 1;
                    octal = octal * 8 + (value[index] - b'0') as u16;
                    digits += 1;
                }
                if octal > u8::MAX as u16 {
                    bail!("octal escape is outside byte range");
                }
                decoded.push(octal as u8);
            }
            _ => bail!("unsupported escape in Git C-quoted value"),
        }
        index += 1;
    }
    decode_utf8(&decoded, "decoded Git value")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nul_records_and_preserves_rich_facts() {
        let input = b"worktree /repo main\0HEAD abc\0branch refs/heads/main\0sparse\0unknown future\0\0worktree /repo\nlinked\0HEAD def\0detached\0locked\0prunable expired\0\0";
        let rows = parse_porcelain_z(input).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].is_main);
        assert_eq!(rows[0].path, PathBuf::from("/repo main"));
        assert_eq!(rows[0].branch_ref.as_deref(), Some("refs/heads/main"));
        assert!(rows[0].is_sparse);
        assert_eq!(rows[1].path, PathBuf::from("/repo\nlinked"));
        assert!(rows[1].is_detached);
        assert_eq!(rows[1].locked, Some(GitAnnotation { reason: None }));
        assert_eq!(
            rows[1].prunable,
            Some(GitAnnotation {
                reason: Some("expired".into())
            })
        );
    }

    #[test]
    fn accepts_bare_and_final_record_without_empty_delimiter() {
        let rows = parse_porcelain_z(b"worktree /srv/repo.git\0bare").unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_bare);
    }

    #[test]
    fn text_mode_decodes_c_quoted_paths_and_reasons() {
        let input = b"worktree \"/repo\\nlinked\"\r\nHEAD abc\r\nbranch refs/heads/feature\r\nlocked \"line\\tbreak\"\r\nprunable \"octal-\\303\\251\"\r\n\r\n";
        let rows = parse_porcelain_text(input).unwrap();
        assert_eq!(rows[0].path, PathBuf::from("/repo\nlinked"));
        assert_eq!(
            rows[0].locked.as_ref().unwrap().reason.as_deref(),
            Some("line\tbreak")
        );
        assert_eq!(
            rows[0].prunable.as_ref().unwrap().reason.as_deref(),
            Some("octal-é")
        );
    }

    #[test]
    fn text_and_nul_modes_match_for_representable_facts() {
        let nul = parse_porcelain_z(
            b"worktree /repo linked\0HEAD abc\0branch refs/heads/feature\0sparse\0locked busy\0prunable\0\0",
        )
        .unwrap();
        let text = parse_porcelain_text(
            b"worktree \"/repo linked\"\nHEAD abc\nbranch refs/heads/feature\nsparse\nlocked \"busy\"\nprunable\n\n",
        )
        .unwrap();
        assert_eq!(text, nul);
        assert_eq!(text[0].prunable, Some(GitAnnotation { reason: None }));
    }

    #[test]
    fn identical_duplicates_are_allowed_but_conflicts_fail() {
        assert!(parse_porcelain_z(b"worktree /repo\0worktree /repo\0\0").is_ok());
        assert!(parse_porcelain_z(b"worktree /repo\0worktree /other\0\0").is_err());
        assert!(parse_porcelain_z(b"worktree /repo\0locked\0locked why\0\0").is_err());
        assert!(parse_porcelain_z(b"worktree /repo\0branch refs/heads/x\0detached\0\0").is_err());
        assert!(parse_porcelain_z(b"worktree /repo\0\0worktree /repo\0\0").is_err());
    }

    #[test]
    fn malformed_nul_inputs_reject_the_whole_parse() {
        for input in [
            b"HEAD abc\0\0".as_slice(),
            b"HEAD abc\0worktree /repo\0\0".as_slice(),
            b"worktree /repo\0\0\0worktree /other\0\0".as_slice(),
            b"worktree \xff\0\0".as_slice(),
            b"worktree /repo\0branch refs/heads/x\0detached\0\0".as_slice(),
            b"worktree \0\0".as_slice(),
            b"worktree /repo\0HEAD \0\0".as_slice(),
            b"worktree /repo\0branch \0\0".as_slice(),
            b"worktree /repo\0future \xff\0\0".as_slice(),
            b"".as_slice(),
        ] {
            assert!(
                parse_porcelain_z(input).is_err(),
                "accepted malformed NUL input: {input:?}"
            );
        }
    }

    #[test]
    fn malformed_text_inputs_reject_the_whole_parse() {
        for input in [
            b"HEAD abc\n\n".as_slice(),
            b"HEAD abc\nworktree /repo\n\n".as_slice(),
            b"worktree \"/repo\\q\"\n\n".as_slice(),
            b"worktree \"/repo\n\n".as_slice(),
            b"worktree \"/repo\"tail\"\n\n".as_slice(),
            b"worktree \"/repo\\377\"\n\n".as_slice(),
            b"worktree \n\n".as_slice(),
            b"worktree /repo\nHEAD\n\n".as_slice(),
            b"worktree /repo\nfuture \xff\n\n".as_slice(),
            b"worktree /repo\0tail\n\n".as_slice(),
            b"worktree \"/repo\\000tail\"\n\n".as_slice(),
            b"worktree /repo\nlocked \"\"\n\n".as_slice(),
            b"worktree /repo\nprunable \n\n".as_slice(),
            b"".as_slice(),
        ] {
            assert!(
                parse_porcelain_text(input).is_err(),
                "accepted malformed text input: {input:?}"
            );
        }
    }
}
