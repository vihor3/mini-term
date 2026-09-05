//! Disposable repository fixtures are for GitHub Actions only. Do not run
//! Cargo, these fixtures, or their Git processes in a local agent session.

use super::*;
use crate::git::GitStatus;

const A: &str = "1111111111111111111111111111111111111111";
const B: &str = "2222222222222222222222222222222222222222";
const C: &str = "3333333333333333333333333333333333333333";
const ZERO: &str = "0000000000000000000000000000000000000000";

fn oid(value: &str) -> ObjectId {
    ObjectId::parse(value).unwrap()
}

fn header(head: Option<&str>) -> String {
    format!(
        "# branch.oid {}\0# branch.head main\0",
        head.unwrap_or("(initial)")
    )
}

fn log_record(hash: &str, parents: &str, subject: &str, body: &str) -> Vec<u8> {
    let payload = format!("{hash}\0{parents}\0Author\x001234567890\0{subject}\0{body}");
    format!("log size {}\n{payload}\0", payload.len()).into_bytes()
}

#[test]
fn object_ids_and_branch_refs_reject_revision_and_option_injection() {
    assert!(ObjectId::parse(A).is_ok());
    assert!(ObjectId::parse(&"a".repeat(64)).is_ok());
    for value in [
        ZERO,
        "HEAD",
        "--all",
        "1234567",
        "HEAD:path",
        "refs/heads/main",
    ] {
        assert!(ObjectId::parse(value).is_err(), "{value}");
    }
    assert!(ObjectId::parse(&"A".repeat(40)).is_err());
    for value in [
        "--upload-pack=other",
        "main~1",
        "main^{tree}",
        "main:path",
        "a..b",
        "@{-1}",
        "a b",
        "a\nb",
        ".hidden",
        "a/.hidden",
        "a.lock",
        "a//b",
        "a/",
        "a.",
        "a\\b",
        "a*b",
        "a?b",
        "a[b",
        "HEAD",
        "@",
        "a\0b",
    ] {
        assert!(GitRef::local(value).is_err(), "{value:?}");
    }
    let branch = GitRef::local("feature/utf8-\u{4e2d}").unwrap();
    assert_eq!(branch.as_str(), "refs/heads/feature/utf8-\u{4e2d}");
    assert!(!branch.is_remote());
    let remote = GitRef::remote("origin/main").unwrap();
    assert_eq!(remote.short_name(), "origin/main");
    assert!(remote.is_remote());
    assert!(GitRef::parse("refs/tags/v1").is_err());
    assert_eq!(
        resolve_ref_plan(&remote).args.last().unwrap(),
        "refs/remotes/origin/main^{commit}"
    );
}

#[test]
fn existing_refs_are_not_reinterpreted_as_branch_creation_input() {
    for name in ["-legacy", "HEAD", "@"] {
        let full = format!("refs/heads/{name}");
        let reference = GitRef::parse(&full).unwrap();
        assert!(GitRef::local(name).is_err());
        let refs = parse_branches(format!("{full}\0{A}\0\0\n").as_bytes(), Some(&oid(A))).unwrap();
        assert_eq!(GitRef::from_branch(&refs[0]).unwrap(), reference);
        assert_eq!(
            resolve_ref_plan(&reference).args.last().unwrap(),
            &format!("{full}^{{commit}}")
        );
        let status = format!("# branch.oid {A}\0# branch.head {name}\0");
        assert_eq!(
            parse_status(status.as_bytes()).unwrap().head.branch,
            Some(reference.clone())
        );
        let inventory = format!("worktree /repo\0HEAD {A}\0branch {full}\0\0");
        assert_eq!(
            parse_worktrees(inventory.as_bytes()).unwrap()[0]
                .branch_ref
                .as_deref(),
            Some(full.as_str())
        );
        for create in [false, true] {
            assert!(worktree_add_plan("/linked", &reference, create, None).is_err());
        }
    }
    let limit = format!(
        "refs/heads/{}",
        "x".repeat(MAX_PATH_BYTES - "refs/heads/".len())
    );
    assert!(GitRef::parse(&limit).is_ok());
    assert!(GitRef::parse(&format!("{limit}x")).is_err());
    for invalid in [
        "refs/heads/../other",
        "refs/heads/a\nb",
        "refs/heads/main^{commit}",
    ] {
        assert!(GitRef::parse(invalid).is_err());
    }
}

#[test]
fn path_plans_preserve_literal_bytes_and_reject_escape_or_empty_selection() {
    let paths: Vec<_> = [
        "-force",
        ":(glob)*",
        "dir/space name",
        "line\nbreak",
        "utf8-\u{4e2d}",
        "back\\slash",
        "[ab]?*.txt",
        "tab\tname",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    let plan = stage_plan(&paths).unwrap();
    assert!(plan.args.iter().any(|arg| arg == "--literal-pathspecs"));
    let separator = plan.args.iter().position(|arg| arg == "--").unwrap();
    assert_eq!(&plan.args[separator + 1..], paths);
    for value in [
        "",
        "/root",
        "../outside",
        "a/../b",
        "a/./b",
        "a//b",
        "a/",
        ".git/config",
        "a\0b",
    ] {
        assert!(stage_plan(&[value.to_string()]).is_err(), "{value:?}");
    }
    assert!(stage_plan(&[]).is_err());
    assert!(stage_plan(&["x".repeat(MAX_PATH_BYTES + 1)]).is_err());
    assert!(stage_plan(&vec!["x".repeat(MAX_PATH_BYTES); 9]).is_err());
}

#[test]
fn capture_rejects_even_record_boundary_truncation_and_unconfirmed_exit() {
    let bytes = header(Some(A));
    let good = CapturedOutput {
        stdout: bytes.as_bytes(),
        stderr: b"",
        exit_code: Some(0),
        timed_out: false,
        stdout_truncated: false,
        stderr_truncated: false,
    };
    let plan = status_plan();
    assert!(
        parse_status(plan.checked_stdout(good).unwrap())
            .unwrap()
            .changes
            .is_empty()
    );
    for bad in [
        CapturedOutput {
            stdout_truncated: true,
            ..good
        },
        CapturedOutput {
            stderr_truncated: true,
            ..good
        },
        CapturedOutput {
            timed_out: true,
            ..good
        },
        CapturedOutput {
            exit_code: None,
            ..good
        },
        CapturedOutput {
            exit_code: Some(128),
            ..good
        },
    ] {
        assert!(plan.checked_stdout(bad).is_err());
    }
    let large = vec![0; MAX_LIST_BYTES + 1];
    assert!(
        plan.checked_stdout(CapturedOutput {
            stdout: &large,
            ..good
        })
        .is_err()
    );
    let stderr = vec![0; DIAGNOSTIC_BYTES + 1];
    assert!(
        plan.checked_stdout(CapturedOutput {
            stderr: &stderr,
            ..good
        })
        .is_err()
    );
}

#[test]
fn literal_pathspecs_stay_global_without_changing_unrelated_hooks() {
    let head = oid(A);
    let path = ":(glob)*".to_string();
    let paths = std::slice::from_ref(&path);
    let DiscardPlan::Git(discard) = discard_plan(&path, Some(&head), true).unwrap() else {
        panic!()
    };
    for (plan, expected_path) in [
        (stage_plan(paths).unwrap(), path.as_str()),
        (stage_all_plan(), "."),
        (unstage_plan(Some(&head), paths).unwrap(), path.as_str()),
        (unstage_plan(None, paths).unwrap(), path.as_str()),
        (tree_entry_plan(&head, &path).unwrap(), path.as_str()),
        (index_entry_plan(&path).unwrap(), path.as_str()),
        (discard, path.as_str()),
    ] {
        let literal = plan
            .args
            .iter()
            .position(|arg| arg == "--literal-pathspecs")
            .unwrap();
        let subcommand = plan
            .args
            .iter()
            .position(|arg| !arg.starts_with('-'))
            .unwrap();
        let separator = plan.args.iter().position(|arg| arg == "--").unwrap();
        assert!(
            literal < subcommand && subcommand < separator,
            "{:?}",
            plan.args
        );
        assert_eq!(plan.args.last().unwrap(), expected_path);
    }
    let branch = GitRef::local("feature").unwrap();
    for plan in [
        commit_plan("commit").unwrap(),
        pull_plan(),
        push_plan(),
        worktree_add_plan("/linked", &branch, true, Some(&head)).unwrap(),
        worktree_remove_plan("/linked", "/main", false).unwrap(),
        worktree_prune_plan(),
    ] {
        assert!(!plan.args.iter().any(|arg| arg == "--literal-pathspecs"));
    }
}

#[test]
fn authority_paths_are_independently_framed_and_never_normalized_lossily() {
    let authority = parse_repository_authority(
        b"/srv/Repo\nwith newline\n",
        b"/srv/common/worktrees/linked\n",
        b"/srv/common\n",
    )
    .unwrap();
    assert_eq!(authority.worktree_root, "/srv/Repo\nwith newline");
    assert!(authority.is_linked_worktree());
    assert!(parse_repository_authority(b"relative\n", b"/git\n", b"/git\n").is_err());
    assert!(parse_repository_authority(b"/repo", b"/git\n", b"/git\n").is_err());
    assert!(parse_repository_authority(b"/\xff\n", b"/git\n", b"/git\n").is_err());
    assert!(parse_repository_authority(b"/repo\0\n", b"/git\n", b"/git\n").is_err());
    for plan in repository_authority_plans() {
        assert_eq!(plan.effect, CommandEffect::ReadOnly);
        assert!(plan.args.iter().any(|arg| arg == "--path-format=absolute"));
    }
}

#[test]
fn status_preserves_partial_index_rename_conflict_untracked_and_ignored_semantics() {
    let bytes = format!(
        "{}1 MM N... 100644 100644 100644 {A} {B} partial\0\
         2 RM N... 100644 100644 100644 {A} {B} R075 new\nname\0old name\0\
         u UU N... 100644 100644 100644 100644 {A} {B} {C} conflicted\0\
         1 .D N... 100644 100644 000000 {A} {A} deleted\0\
         1 .M S.MU 160000 160000 160000 {A} {A} submodule\0\
         ? :(glob)*\0! ignored/\0",
        header(Some(A))
    );
    let parsed = parse_status(bytes.as_bytes()).unwrap();
    assert_eq!(parsed.head.oid, Some(oid(A)));
    assert_eq!(parsed.changes.len(), 6);
    let partial = &parsed.changes[0];
    assert_eq!(partial.staged_status, Some(GitStatus::Modified));
    assert_eq!(partial.unstaged_status, Some(GitStatus::Modified));
    assert_eq!(parsed.changes[1].path, "new\nname");
    assert_eq!(parsed.changes[1].old_path.as_deref(), Some("old name"));
    assert_eq!(parsed.changes[1].staged_status, Some(GitStatus::Renamed));
    assert_eq!(parsed.changes[2].staged_status, Some(GitStatus::Conflicted));
    assert_eq!(
        parsed.changes[2].unstaged_status,
        Some(GitStatus::Conflicted)
    );
    assert_eq!(parsed.changes[3].unstaged_status, Some(GitStatus::Deleted));
    assert_eq!(parsed.changes[5].path, ":(glob)*");
    assert_eq!(
        parsed.changes[5].unstaged_status,
        Some(GitStatus::Untracked)
    );
}

#[test]
fn status_distinguishes_unborn_detached_sha256_and_intent_to_add() {
    let unborn = format!("{}? new\0", header(None));
    let status = parse_status(unborn.as_bytes()).unwrap();
    assert!(status.head.oid.is_none());
    assert_eq!(status.changes[0].unstaged_status, Some(GitStatus::Added));
    let detached = format!("# branch.oid {A}\0# branch.head (detached)\0");
    assert!(
        parse_status(detached.as_bytes())
            .unwrap()
            .head
            .branch
            .is_none()
    );
    let sha256 = "a".repeat(64);
    assert_eq!(
        parse_status(header(Some(&sha256)).as_bytes())
            .unwrap()
            .head
            .oid,
        Some(oid(&sha256))
    );
    let intent = format!(
        "{}1 .A N... 000000 000000 100644 {ZERO} {ZERO} intent\0",
        header(Some(A))
    );
    assert_eq!(
        parse_status(intent.as_bytes()).unwrap().changes[0].unstaged_status,
        Some(GitStatus::Added)
    );
}

#[test]
fn status_preserves_untracked_directories_outside_actionable_file_rows() {
    let directories = ["nested/", "space name/line\nback\\slash/", ":(glob)*/"];
    let mut bytes = format!(
        "{}1 MM N... 100644 100644 100644 {A} {B} parent\0? loose\0",
        header(Some(A))
    );
    for directory in directories {
        bytes.push_str(&format!("? {directory}\0"));
    }
    bytes.push_str("! ignored/\0");
    let status = parse_status(bytes.as_bytes()).unwrap();
    assert_eq!(status.untracked_directories, directories);
    assert_eq!(status.changes.len(), 2);
    assert_eq!(status.changes[0].path, "parent");
    assert_eq!(status.changes[0].staged_status, Some(GitStatus::Modified));
    assert_eq!(status.changes[0].unstaged_status, Some(GitStatus::Modified));
    assert_eq!(status.changes[1].path, "loose");
    assert_eq!(
        status.changes[1].unstaged_status,
        Some(GitStatus::Untracked)
    );
    for directory in &status.untracked_directories {
        assert!(stage_plan(std::slice::from_ref(directory)).is_err());
        assert!(unstage_plan(Some(&oid(A)), std::slice::from_ref(directory)).is_err());
        assert!(discard_plan(directory, Some(&oid(A)), false).is_err());
        for staged in [false, true] {
            assert!(working_diff_plan(Some(&oid(A)), directory, None, staged).is_err());
        }
        assert!(commit_diff_plan(&oid(A), None, directory, None).is_err());
    }
    let unborn = parse_status(format!("{}? nested/\0? new\0", header(None)).as_bytes()).unwrap();
    assert_eq!(unborn.untracked_directories, ["nested/"]);
    assert_eq!(unborn.changes.len(), 1);
    assert_eq!(unborn.changes[0].unstaged_status, Some(GitStatus::Added));
    let all = stage_all_plan();
    assert_eq!(
        &all.args[all.args.len() - 4..],
        &["add", "--all", "--", "."]
    );
}

#[test]
fn status_rejects_malformed_and_duplicate_directory_paths() {
    for records in [
        "? /\0",
        "? //\0",
        "? /absolute/\0",
        "? nested//\0",
        "? nested//child/\0",
        "? ../outside/\0",
        "? nested/./child/\0",
        "? nested/../child/\0",
        "? .git/\0",
        "? nested/.git/\0",
        "? nested/",
        "? nested/\0? nested/\0",
        "? nested/\0? nested\0",
        "? nested\0? nested/\0",
    ] {
        assert!(
            parse_status(format!("{}{records}", header(Some(A))).as_bytes()).is_err(),
            "{records:?}"
        );
    }
    let tracked = format!("1 .M N... 100644 100644 100644 {A} {A} nested\0");
    for records in [
        format!("{tracked}? nested/\0"),
        format!("? nested/\0{tracked}"),
    ] {
        assert!(parse_status(format!("{}{records}", header(Some(A))).as_bytes()).is_err());
    }
    let mut invalid = header(Some(A)).into_bytes();
    invalid.extend_from_slice(b"? bad\xff/\0");
    assert!(parse_status(&invalid).is_err());
    let longest = format!("{}/", "x".repeat(MAX_PATH_BYTES - 1));
    let status = parse_status(format!("{}? {longest}\0", header(Some(A))).as_bytes()).unwrap();
    assert_eq!(status.untracked_directories, [longest.clone()]);
    assert!(parse_status(format!("{}? x{longest}\0", header(Some(A))).as_bytes()).is_err());
}

#[test]
fn status_counts_files_and_directories_against_one_record_limit() {
    let mut bytes = header(Some(A));
    for index in 0..MAX_RECORDS {
        let suffix = if index % 2 == 0 { "" } else { "/" };
        bytes.push_str(&format!("? entry{index}{suffix}\0"));
    }
    let status = parse_status(bytes.as_bytes()).unwrap();
    assert_eq!(status.changes.len(), MAX_RECORDS / 2);
    assert_eq!(status.untracked_directories.len(), MAX_RECORDS / 2);
    for extra in ["? overflow\0", "? overflow/\0"] {
        assert!(parse_status(format!("{bytes}{extra}").as_bytes()).is_err());
    }
}

#[test]
fn status_rejects_malformed_truncated_non_utf8_and_duplicate_records() {
    for bytes in [
        Vec::new(),
        b"? file\0".to_vec(),
        header(Some(A)).into_bytes()[..10].to_vec(),
        format!("{}? file", header(Some(A))).into_bytes(),
        format!("{}? file\0? file\0", header(Some(A))).into_bytes(),
        format!(
            "{}2 R. N... 100644 100644 100644 {A} {B} R100 new\0",
            header(Some(A))
        )
        .into_bytes(),
        format!(
            "{}2 R. N... 100644 100644 100644 {A} {B} \u{4e2d} new\0old\0",
            header(Some(A))
        )
        .into_bytes(),
        format!(
            "{}1 .Z N... 100644 100644 100644 {A} {B} path\0",
            header(Some(A))
        )
        .into_bytes(),
        format!(
            "{}1 .. N... 100644 100644 100644 {A} {B} path\0",
            header(Some(A))
        )
        .into_bytes(),
        format!(
            "{}u ZZ N... 100644 100644 100644 100644 {A} {B} {C} path\0",
            header(Some(A))
        )
        .into_bytes(),
        format!("{}# branch.oid {B}\0", header(Some(A))).into_bytes(),
        b"# branch.oid (initial)\0# branch.head (detached)\0".to_vec(),
    ] {
        assert!(parse_status(&bytes).is_err(), "{bytes:?}");
    }
    let mut invalid = header(Some(A)).into_bytes();
    invalid.extend_from_slice(b"? bad\xff\0");
    assert!(parse_status(&invalid).is_err());
}

#[test]
fn repository_captures_reject_mixed_object_formats_even_without_head() {
    let sha256 = "a".repeat(64);
    let zero256 = "0".repeat(64);
    let status = format!(
        "{}1 A. N... 000000 100644 100644 {ZERO} {A} first\0\
         1 A. N... 000000 100644 100644 {zero256} {sha256} second\0",
        header(None)
    );
    assert!(parse_status(status.as_bytes()).is_err());
    let refs = format!("refs/heads/a\0{A}\0\0\nrefs/heads/b\0{sha256}\0\0\n");
    assert!(parse_branches(refs.as_bytes(), None).is_err());
    assert!(
        parse_branches(
            format!("refs/heads/b\0{sha256}\0\0\n").as_bytes(),
            Some(&oid(A))
        )
        .is_err()
    );
    let mut log = log_record(A, "", "first", "");
    log.extend(log_record(&sha256, "", "second", ""));
    assert!(parse_log(&log).is_err());
    let worktrees = format!(
        "worktree /main\0HEAD {A}\0branch refs/heads/main\0\0\
         worktree /linked\0HEAD {sha256}\0detached\0\0"
    );
    assert!(parse_worktrees(worktrees.as_bytes()).is_err());
}

#[test]
fn branch_parser_retains_local_head_parity_and_excludes_remote_head() {
    let bytes = format!(
        "refs/heads/main\0{A}\0\0\nrefs/heads/same\0{A}\0\0\n\
         refs/remotes/origin/HEAD\0{A}\0refs/remotes/origin/main\0\n\
         refs/remotes/origin/main\0{A}\0\0\n"
    );
    let branches = parse_branches(bytes.as_bytes(), Some(&oid(A))).unwrap();
    assert_eq!(branches.len(), 3);
    assert!(branches[0].is_head && branches[1].is_head);
    assert!(branches[2].is_remote && !branches[2].is_head);
    assert_eq!(branches[2].name, "origin/main");
    assert!(parse_branches(b"", None).unwrap().is_empty());
    assert!(parse_branches(&bytes.as_bytes()[..bytes.len() - 1], None).is_err());
    assert!(parse_branches(format!("{bytes}{bytes}").as_bytes(), None).is_err());
}

#[test]
fn history_has_length_framing_all_parents_and_no_implicit_head_at_end() {
    let mut bytes = log_record(A, &format!("{B} {C}"), "subject", "line 1\n\nline 2\n");
    bytes.extend(log_record(B, "", "root", ""));
    let log = parse_log(&bytes).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].parent_hashes, [B, C]);
    assert_eq!(log[0].body.as_deref(), Some("line 1\n\nline 2\n"));
    assert_eq!(log[0].timestamp, 1_234_567_890);
    assert!(log[1].body.is_none());
    let plan = log_plan(&[oid(B), oid(C)], 30).unwrap().unwrap();
    assert!(plan.args.iter().any(|arg| arg == B));
    assert!(plan.args.iter().any(|arg| arg == C));
    assert!(
        !plan
            .args
            .iter()
            .any(|arg| arg.contains("skip") || arg == "--first-parent")
    );
    assert!(log_plan(&[], 30).unwrap().is_none());
    assert!(log_plan(&[oid(A)], 0).is_err());
    assert!(log_plan(&[oid(A)], MAX_LOG_COMMITS + 1).is_err());
    assert_eq!(
        parse_commit_parents(format!("{A} {B} {C}\n").as_bytes(), &oid(A)).unwrap(),
        [oid(B), oid(C)]
    );
    assert!(parse_commit_parents(format!("{A}\n").as_bytes(), &oid(B)).is_err());
    assert!(parse_commit_parents(format!("{A} \n").as_bytes(), &oid(A)).is_err());
}

#[test]
fn history_rejects_partial_payloads_and_embedded_record_injection() {
    let valid = log_record(A, B, "subject", "body\n");
    for end in 1..valid.len() {
        assert!(parse_log(&valid[..end]).is_err(), "prefix {end}");
    }
    let injected = log_record(
        A,
        B,
        "subject",
        &format!("body\0{C}\0\0Author\x001\0forged\0body"),
    );
    assert!(parse_log(&injected).is_err());
    assert!(parse_log(b"log size 99999999999999999999999999999\n").is_err());
    let mut duplicate = valid.clone();
    duplicate.extend(valid);
    assert!(parse_log(&duplicate).is_err());
    assert!(parse_log(&log_record(A, B, "bad\nsubject", "")).is_err());
}

#[test]
fn commit_files_and_exact_object_lookups_preserve_paths_and_missing_sides() {
    let files =
        parse_commit_files(b"A\0space name\0D\0removed\0R100\0old\nname\0new\nname\0").unwrap();
    assert_eq!(files[2].path, "new\nname");
    assert_eq!(files[2].old_path.as_deref(), Some("old\nname"));
    assert_eq!(files[1].status, "deleted");
    assert!(parse_commit_files(b"R100\0old\0").is_err());
    assert!(parse_commit_files(b"A\0x\0A\0x\0").is_err());
    let tree = format!("100644 blob {A}      12\tline\nname\0");
    let entry = parse_tree_entry(tree.as_bytes(), "line\nname")
        .unwrap()
        .unwrap();
    assert_eq!(entry.size, Some(12));
    assert_eq!(entry.kind, EntryKind::Blob);
    assert!(parse_tree_entry(tree.as_bytes(), "other").is_err());
    assert!(parse_tree_entry(format!("100644\nblob {A} 12\tx\0").as_bytes(), "x").is_err());
    assert!(parse_tree_entry(b"", "missing").unwrap().is_none());
    let gitlink = format!("160000 commit {A}       -\tmodule\0");
    assert_eq!(
        parse_tree_entry(gitlink.as_bytes(), "module")
            .unwrap()
            .unwrap()
            .kind,
        EntryKind::Commit
    );
    let index = format!("100644 {A} 0\t:(glob)*\0");
    assert_eq!(
        parse_index_entry(index.as_bytes(), ":(glob)*")
            .unwrap()
            .unwrap()
            .oid,
        oid(A)
    );
    assert!(parse_index_entry(b"", "missing").unwrap().is_none());
    assert!(parse_index_entry(format!("100644 {A} 2\tx\0").as_bytes(), "x").is_err());
    assert!(parse_index_entry(format!("160000 {A} 0\tx\0").as_bytes(), "x").is_err());
    assert_eq!(parse_blob_size(b"0\n").unwrap(), 0);
    assert!(parse_blob_size(b"12").is_err());
    assert!(parse_blob_size(b"+12\n").is_err());
}

#[test]
fn exact_lookup_absence_requires_complete_success_and_single_matching_path() {
    let empty = CapturedOutput {
        stdout: b"",
        stderr: b"",
        exit_code: Some(0),
        timed_out: false,
        stdout_truncated: false,
        stderr_truncated: false,
    };
    for plan in [
        tree_entry_plan(&oid(A), "file").unwrap(),
        index_entry_plan("file").unwrap(),
    ] {
        assert!(plan.checked_stdout(empty).unwrap().is_empty());
        for invalid in [
            CapturedOutput {
                exit_code: Some(128),
                ..empty
            },
            CapturedOutput {
                stdout_truncated: true,
                ..empty
            },
            CapturedOutput {
                stderr_truncated: true,
                ..empty
            },
            CapturedOutput {
                timed_out: true,
                ..empty
            },
            CapturedOutput {
                exit_code: None,
                ..empty
            },
        ] {
            assert!(plan.checked_stdout(invalid).is_err());
        }
    }
    for bad in [
        format!("100644 blob {A}       1\tfile"),
        format!("100644 blob {A}       1\tfile/child\0"),
        format!("100644 blob {A}       1\tfile\0\0"),
        format!(" 100644 blob {A}       1\tfile\0"),
        format!("100644  blob {A}       1\tfile\0"),
        format!("100644 blob {A}       1 \tfile\0"),
        format!("{}\tfile\0", "field ".repeat(MAX_PATH_BYTES)),
    ] {
        assert!(parse_tree_entry(bad.as_bytes(), "file").is_err());
    }
    let tree = format!("040000 tree {A}       -\tfile\0");
    assert_eq!(
        parse_tree_entry(tree.as_bytes(), "file")
            .unwrap()
            .unwrap()
            .kind,
        EntryKind::Tree
    );
    for bad in [
        format!("100644 {A} 0\tfile/child\0"),
        format!("100644 {A} 0\tfile\0"),
    ] {
        let duplicate = format!("{bad}{bad}");
        assert!(parse_index_entry(duplicate.as_bytes(), "file").is_err());
    }
    for stage in [1, 2, 3] {
        let conflict = format!("100644 {A} {stage}\tfile\0");
        assert!(parse_index_entry(conflict.as_bytes(), "file").is_err());
    }
    let extra_fields = format!("100644 {A} 0 {}\tfile\0", "field ".repeat(MAX_PATH_BYTES));
    assert!(parse_index_entry(extra_fields.as_bytes(), "file").is_err());
    let invalid = format!("100644 {A} 0\t");
    let mut invalid = invalid.into_bytes();
    invalid.extend_from_slice(b"file\xff\0");
    assert!(parse_index_entry(&invalid, "file").is_err());
}

#[test]
fn diff_plans_compare_both_modes_to_head_and_commit_to_first_parent() {
    let working = working_diff_plan(Some(&oid(A)), "new", Some("old"), false).unwrap();
    let staged = working_diff_plan(Some(&oid(A)), "new", Some("old"), true).unwrap();
    assert_eq!(working.old, staged.old);
    assert_eq!(
        working.old,
        BlobLookup::Tree {
            commit: oid(A),
            path: "old".to_string()
        }
    );
    assert_eq!(
        working.new,
        BlobLookup::Worktree {
            path: "new".to_string()
        }
    );
    assert_eq!(
        staged.new,
        BlobLookup::Index {
            path: "new".to_string()
        }
    );
    assert_eq!(
        working_diff_plan(None, "new", None, false).unwrap().old,
        BlobLookup::Empty
    );
    assert_eq!(
        commit_diff_plan(&oid(B), Some(&oid(A)), "new", Some("old"))
            .unwrap()
            .old,
        staged.old
    );
    assert_eq!(
        commit_diff_plan(&oid(A), None, "new", None).unwrap().old,
        BlobLookup::Empty
    );
    let root = commit_files_plan(&oid(A), None);
    assert!(root.args.iter().any(|arg| arg == "--root"));
    let merge = commit_files_plan(&oid(A), Some(&oid(B)));
    assert!(!merge.args.iter().any(|arg| arg == "--root"));
    assert_eq!(&merge.args[merge.args.len() - 3..], &[B, A, "--"]);
}

#[test]
fn byte_diff_reuses_text_builder_and_bounds_both_sides() {
    let actual = build_diff(DiffContent::Bytes(b"old\n"), DiffContent::Bytes(b"new\n"));
    let expected = crate::git::diff_two_texts("old\n".to_string(), "new\n".to_string());
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let removed = build_diff(DiffContent::Bytes(b"old\n"), DiffContent::Missing);
    assert!(removed.new_content.is_empty() && !removed.hunks.is_empty());
    for binary in [b"has\0nul".as_slice(), b"invalid\xff".as_slice()] {
        assert!(build_diff(DiffContent::Bytes(binary), DiffContent::Missing).is_binary);
        assert!(build_diff(DiffContent::Missing, DiffContent::Bytes(binary)).is_binary);
    }
    let too_large = vec![b'x'; MAX_BLOB_BYTES + 1];
    assert!(build_diff(DiffContent::Bytes(&too_large), DiffContent::Missing).too_large);
    assert!(build_diff(DiffContent::Missing, DiffContent::Bytes(&too_large)).too_large);
    assert!(build_diff(DiffContent::TooLarge, DiffContent::Bytes(b"\0")).too_large);
}

#[test]
fn mutations_have_explicit_unborn_discard_and_host_configuration_semantics() {
    let paths = [":(glob)*".to_string()];
    let unborn = unstage_plan(None, &paths).unwrap();
    assert!(unborn.args.iter().any(|arg| arg == "--cached"));
    assert!(
        unstage_all_plan(None)
            .args
            .iter()
            .any(|arg| arg == "--empty")
    );
    let born = unstage_all_plan(Some(&oid(A)));
    assert_eq!(
        &born.args[born.args.len() - 4..],
        &["reset", "--mixed", "HEAD", "--"]
    );
    assert!(!born.args.iter().any(|arg| arg == "--hard" || arg == A));
    assert!(matches!(
        discard_plan("new", None, false).unwrap(),
        DiscardPlan::RemoveUntrackedFile { .. }
    ));
    let DiscardPlan::Git(discard) = discard_plan("tracked", Some(&oid(A)), true).unwrap() else {
        panic!()
    };
    assert!(discard.args.iter().any(|arg| arg == "--staged"));
    assert!(discard.args.iter().any(|arg| arg == "--worktree"));
    let message = "--amend\n\n$(touch /not-a-command)";
    let commit = commit_plan(message).unwrap();
    assert_eq!(
        &commit.args[commit.args.len() - 3..],
        &["commit", "-m", message]
    );
    assert!(commit_plan(" \n").is_err());
    assert!(commit_plan("nul\0").is_err());
    for plan in [commit, pull_plan(), push_plan()] {
        assert_eq!(plan.effect, CommandEffect::Mutation);
        assert!(
            !plan
                .args
                .iter()
                .any(|arg| arg == "--no-verify" || arg == "--force" || arg == "-c")
        );
    }
}

#[test]
fn worktree_plans_reuse_posix_inventory_and_exclude_main_removal() {
    let bytes = format!(
        "worktree /srv/Repo\0HEAD {A}\0branch refs/heads/main\0\0worktree /srv/repo\0HEAD {B}\0detached\0\0"
    );
    let worktrees = parse_worktrees(bytes.as_bytes()).unwrap();
    assert_eq!(worktrees.len(), 2);
    assert!(worktrees[0].is_main);
    assert!(worktrees[1].is_detached);
    assert!(parse_worktrees(b"").is_err());
    assert!(parse_worktrees(b"worktree /repo").is_err());
    assert!(parse_worktrees(b"worktree /repo\0").is_err());
    let unusual = format!("worktree /srv/Case\\\0HEAD {A}\0branch refs/heads/main\0\0");
    let projected = project_worktrees(&parse_worktrees(unusual.as_bytes()).unwrap()).unwrap();
    assert_eq!(projected[0].path, "/srv/Case\\");
    assert_eq!(projected[0].name, "Case\\");
    let branch = GitRef::local("feature").unwrap();
    let create = worktree_add_plan("/srv/new\nworktree", &branch, true, Some(&oid(A))).unwrap();
    assert_eq!(
        &create.args[create.args.len() - 5..],
        &["-b", "feature", "--", "/srv/new\nworktree", A]
    );
    assert!(worktree_add_plan("relative", &branch, true, None).is_err());
    assert!(
        worktree_add_plan(
            "/srv/new",
            &GitRef::remote("origin/main").unwrap(),
            false,
            None
        )
        .is_err()
    );
    assert!(worktree_remove_plan("/srv/main", "/srv/main", true).is_err());
    assert!(worktree_remove_plan("/", "/srv/main", true).is_err());
    let remove = worktree_remove_plan("/srv/linked", "/srv/main", true).unwrap();
    assert_eq!(
        &remove.args[remove.args.len() - 3..],
        &["--force", "--", "/srv/linked"]
    );
}

#[test]
fn worktree_records_require_head_or_bare_without_erasing_prunable_rows() {
    for invalid in [
        b"worktree /repo\0\0".as_slice(),
        b"worktree /repo\0branch refs/heads/main\0\0".as_slice(),
        b"worktree /repo\0detached\0\0".as_slice(),
        b"worktree /repo\0bare\0branch refs/heads/main\0\0".as_slice(),
        b"worktree /repo\0bare\0detached\0\0".as_slice(),
    ] {
        assert!(parse_worktrees(invalid).is_err());
    }
    let invalid = format!("worktree /repo\0bare\0HEAD {A}\0\0");
    assert!(parse_worktrees(invalid.as_bytes()).is_err());
    let bare = parse_worktrees(b"worktree /repo.git\0bare\0\0").unwrap();
    assert!(bare[0].is_main && bare[0].is_bare);
    let unborn = format!("worktree /repo\0HEAD {ZERO}\0branch refs/heads/main\0\0");
    assert_eq!(
        parse_worktrees(unborn.as_bytes()).unwrap()[0]
            .head
            .as_deref(),
        Some(ZERO)
    );
    let prunable = format!(
        "worktree /repo\0HEAD {A}\0branch refs/heads/main\0\0\
         worktree /missing\0HEAD {ZERO}\0prunable broken HEAD\0\0"
    );
    let rows = parse_worktrees(prunable.as_bytes()).unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[1].prunable.is_some());
}

#[cfg(unix)]
mod actions_fixtures {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture {
        root: PathBuf,
        repo: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "mini-term-git-cli-{}-{nonce}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            let repo = root.join("repo");
            std::fs::create_dir(&repo).unwrap();
            let fixture = Self { root, repo };
            fixture.raw(&["init", "--initial-branch=main"]);
            fixture
        }

        fn output_at(&self, cwd: &Path, args: &[String]) -> Output {
            self.output_at_with_date(cwd, args, "2001-01-01T00:00:00Z")
        }

        fn output_at_with_date(&self, cwd: &Path, args: &[String], date: &str) -> Output {
            assert!(cwd.starts_with(&self.root));
            let mut command = Command::new("git");
            command
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("HOME", &self.root)
                .env("XDG_CONFIG_HOME", &self.root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "Fixture Author")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Fixture Author")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date)
                .env("LC_ALL", "C")
                .args(args)
                .current_dir(cwd)
                .stdin(Stdio::null());
            command.output().unwrap()
        }

        fn raw(&self, args: &[&str]) -> Vec<u8> {
            self.raw_at(&self.repo, args)
        }

        fn raw_at(&self, cwd: &Path, args: &[&str]) -> Vec<u8> {
            let args: Vec<_> = args.iter().map(|arg| (*arg).to_string()).collect();
            let output = self.output_at(cwd, &args);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        }

        fn run_at(&self, cwd: &Path, plan: &GitCommand) -> Vec<u8> {
            let output = self.output_at(cwd, &plan.args);
            plan.checked_stdout(CapturedOutput {
                stdout: &output.stdout,
                stderr: &output.stderr,
                exit_code: output.status.code(),
                timed_out: false,
                stdout_truncated: false,
                stderr_truncated: false,
            })
            .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)))
            .to_vec()
        }

        fn run(&self, plan: &GitCommand) -> Vec<u8> {
            self.run_at(&self.repo, plan)
        }

        fn file(&self, path: &str, bytes: &[u8]) {
            validate_repo_path(path).unwrap();
            let target = self.repo.join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, bytes).unwrap();
        }

        fn status(&self) -> RepositoryStatus {
            parse_status(&self.run(&status_plan())).unwrap()
        }

        fn commit(&self, message: &str) -> ObjectId {
            self.run(&stage_all_plan());
            self.run(&commit_plan(message).unwrap());
            self.status().head.oid.unwrap()
        }

        fn content(&self, lookup: &BlobLookup) -> Option<Vec<u8>> {
            let oid = match lookup {
                BlobLookup::Empty => return None,
                BlobLookup::Tree { commit, path } => {
                    let entry =
                        parse_tree_entry(&self.run(&tree_entry_plan(commit, path).unwrap()), path)
                            .unwrap()?;
                    assert_eq!(entry.kind, EntryKind::Blob);
                    assert!(entry.size.unwrap() <= MAX_BLOB_BYTES as u64);
                    entry.oid
                }
                BlobLookup::Index { path } => {
                    parse_index_entry(&self.run(&index_entry_plan(path).unwrap()), path)
                        .unwrap()?
                        .oid
                }
                BlobLookup::Worktree { path } => {
                    return match std::fs::read(self.repo.join(path)) {
                        Ok(bytes) => Some(bytes),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                        Err(error) => panic!("{error}"),
                    };
                }
            };
            assert!(
                parse_blob_size(&self.run(&blob_size_plan(&oid))).unwrap() <= MAX_BLOB_BYTES as u64
            );
            Some(self.run(&blob_plan(&oid)))
        }

        fn diff(&self, plan: &DiffPlan) -> GitDiffResult {
            let old = self.content(&plan.old);
            let new = self.content(&plan.new);
            build_diff(
                old.as_deref()
                    .map_or(DiffContent::Missing, DiffContent::Bytes),
                new.as_deref()
                    .map_or(DiffContent::Missing, DiffContent::Bytes),
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn actual_plans_preserve_unborn_hostile_filenames_and_index_only_unstage() {
        let fixture = Fixture::new();
        assert!(fixture.status().head.oid.is_none());
        let paths: Vec<_> = [
            "space name",
            "line\nbreak",
            "utf8-\u{4e2d}",
            "-leading",
            ":(glob)*",
            "[a]?",
            "back\\slash",
            "tab\tfile",
            "nested/dir/file",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        for path in &paths {
            fixture.file(path, b"original\n");
        }
        fixture.run(&stage_plan(&[":(glob)*".to_string()]).unwrap());
        assert_eq!(
            fixture
                .status()
                .changes
                .iter()
                .filter(|entry| entry.staged_status.is_some())
                .count(),
            1
        );
        fixture.run(&stage_all_plan());
        let status = fixture.status();
        assert_eq!(status.changes.len(), paths.len());
        assert!(
            status
                .changes
                .iter()
                .all(|entry| entry.staged_status == Some(GitStatus::Added))
        );
        fixture.run(&unstage_plan(None, &[":(glob)*".to_string()]).unwrap());
        assert!(
            parse_index_entry(
                &fixture.run(&index_entry_plan(":(glob)*").unwrap()),
                ":(glob)*"
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(
            std::fs::read(fixture.repo.join(":(glob)*")).unwrap(),
            b"original\n"
        );
        fixture.run(&unstage_all_plan(None));
        assert!(
            fixture
                .status()
                .changes
                .iter()
                .all(|entry| entry.staged_status.is_none())
        );
        fixture.file("unborn-discard", b"unborn\n");
        fixture.run(&stage_plan(&["unborn-discard".to_string()]).unwrap());
        let DiscardPlan::Git(discard) = discard_plan("unborn-discard", None, true).unwrap() else {
            panic!()
        };
        fixture.run(&discard);
        assert!(!fixture.repo.join("unborn-discard").exists());
        fixture.file(".gitignore", b"ignored\n");
        fixture.file("ignored", b"sentinel\n");
        fixture.run(&stage_all_plan());
        assert!(
            !fixture
                .status()
                .changes
                .iter()
                .any(|entry| entry.path == "ignored")
        );
        fixture.run(&commit_plan("root\n\nmessage body\n").unwrap());
        let head = fixture.status().head.oid.unwrap();
        let roots = parse_commit_files(&fixture.run(&commit_files_plan(&head, None))).unwrap();
        assert_eq!(roots.len(), paths.len() + 1);
        for path in &paths {
            assert!(roots.iter().any(|entry| entry.path == *path));
            let diff = fixture.diff(&commit_diff_plan(&head, None, path, None).unwrap());
            assert_eq!(diff.old_content, "");
            assert_eq!(diff.new_content, "original\n");
        }
    }

    #[test]
    fn actual_nested_repository_status_retains_modified_parent_and_explicit_stage_all() {
        let fixture = Fixture::new();
        fixture.file("parent", b"original\n");
        fixture.commit("parent root");
        let nested = fixture.repo.join("nested");
        fixture.raw(&["init", "--initial-branch=main", nested.to_str().unwrap()]);
        std::fs::write(nested.join("child"), b"nested contents\n").unwrap();
        fixture.run_at(&nested, &stage_all_plan());
        fixture.run_at(&nested, &commit_plan("nested root").unwrap());
        fixture.file("parent", b"modified\n");

        let status = parse_status(&fixture.run(&status_plan())).unwrap();
        assert_eq!(status.untracked_directories, ["nested/"]);
        assert_eq!(status.changes.len(), 1);
        assert_eq!(status.changes[0].path, "parent");
        assert_eq!(status.changes[0].unstaged_status, Some(GitStatus::Modified));
        let directory = &status.untracked_directories[0];
        assert!(stage_plan(std::slice::from_ref(directory)).is_err());
        assert!(discard_plan(directory, status.head.oid.as_ref(), false).is_err());
        assert!(working_diff_plan(status.head.oid.as_ref(), directory, None, false).is_err());
        fixture.run(&stage_plan(&["parent".to_string()]).unwrap());
        let staged = fixture.status();
        assert_eq!(staged.untracked_directories, ["nested/"]);
        assert_eq!(staged.changes[0].staged_status, Some(GitStatus::Modified));

        fixture.run(&stage_all_plan());
        let all = fixture.status();
        assert!(all.untracked_directories.is_empty());
        assert_eq!(all.changes.len(), 2);
        assert!(
            all.changes
                .iter()
                .any(|row| row.path == "nested" && row.staged_status == Some(GitStatus::Added))
        );
        assert!(
            all.changes
                .iter()
                .any(|row| row.path == "parent" && row.staged_status == Some(GitStatus::Modified))
        );
        assert_eq!(
            std::fs::read(nested.join("child")).unwrap(),
            b"nested contents\n"
        );
    }

    #[test]
    fn actual_existing_refs_remain_qualified_through_read_plans() {
        let fixture = Fixture::new();
        fixture.file("tracked", b"root\n");
        let head = fixture.commit("root");
        for name in ["-legacy", "HEAD", "@"] {
            let full = format!("refs/heads/{name}");
            fixture.raw(&["update-ref", &full, head.as_str()]);
        }
        fixture.file("tracked", b"main\n");
        fixture.commit("main differs from legacy refs");
        let branches = parse_branches(
            &fixture.run(&branches_plan()),
            fixture.status().head.oid.as_ref(),
        )
        .unwrap();
        for name in ["-legacy", "HEAD", "@"] {
            let branch = branches.iter().find(|branch| branch.name == name).unwrap();
            let reference = GitRef::from_branch(branch).unwrap();
            let resolved = parse_object_id(&fixture.run(&resolve_ref_plan(&reference))).unwrap();
            assert_eq!(resolved, head);
            let history =
                parse_log(&fixture.run(&log_plan(&[resolved], 30).unwrap().unwrap())).unwrap();
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].hash, head.as_str());
        }
        fixture.raw(&["symbolic-ref", "HEAD", "refs/heads/-legacy"]);
        assert_eq!(
            fixture.status().head.branch.unwrap().as_str(),
            "refs/heads/-legacy"
        );
        let inventory = parse_worktrees(&fixture.run(&worktree_list_plan())).unwrap();
        assert_eq!(
            inventory[0].branch_ref.as_deref(),
            Some("refs/heads/-legacy")
        );
    }

    #[test]
    fn actual_commit_hook_keeps_its_own_git_pathspec_semantics() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new();
        fixture.file("tracked", b"root\n");
        let root = fixture.commit("root");
        let hooks = fixture.root.join("hooks");
        std::fs::create_dir(&hooks).unwrap();
        let pre_commit = hooks.join("pre-commit");
        std::fs::write(
            &pre_commit,
            b"#!/bin/sh\nset -eu\ngit add -- 'generated-*.txt'\n",
        )
        .unwrap();
        std::fs::set_permissions(&pre_commit, std::fs::Permissions::from_mode(0o700)).unwrap();
        fixture.raw(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
        fixture.file("tracked", b"changed\n");
        fixture.file("generated-one.txt", b"hook-selected\n");
        fixture.run(&stage_plan(&["tracked".to_string()]).unwrap());
        fixture.run(&commit_plan("with normal hook Git behavior").unwrap());
        let head = fixture.status().head.oid.unwrap();
        let diff =
            fixture.diff(&commit_diff_plan(&head, Some(&root), "generated-one.txt", None).unwrap());
        assert_eq!(diff.new_content, "hook-selected\n");
        assert!(fixture.status().changes.is_empty());
    }

    #[test]
    fn actual_sha256_status_history_and_blob_plans_use_full_ids() {
        let fixture = Fixture::new();
        let repo = fixture.root.join("sha256");
        fixture.raw_at(
            &fixture.root,
            &[
                "init",
                "--object-format=sha256",
                "--initial-branch=main",
                repo.to_str().unwrap(),
            ],
        );
        let unborn = parse_status(&fixture.run_at(&repo, &status_plan())).unwrap();
        assert!(unborn.head.oid.is_none());
        let path = "line\n:(glob)*";
        std::fs::write(repo.join(path), b"sha256\n").unwrap();
        fixture.run_at(&repo, &stage_plan(&[path.to_string()]).unwrap());
        let staged = parse_status(&fixture.run_at(&repo, &status_plan())).unwrap();
        assert_eq!(staged.changes[0].staged_status, Some(GitStatus::Added));
        fixture.run_at(&repo, &commit_plan("sha256 root").unwrap());
        let head = parse_status(&fixture.run_at(&repo, &status_plan()))
            .unwrap()
            .head
            .oid
            .unwrap();
        assert_eq!(head.as_str().len(), 64);
        assert!(
            parse_commit_parents(&fixture.run_at(&repo, &commit_parents_plan(&head)), &head)
                .unwrap()
                .is_empty()
        );
        let log = parse_log(&fixture.run_at(
            &repo,
            &log_plan(std::slice::from_ref(&head), 30).unwrap().unwrap(),
        ))
        .unwrap();
        assert_eq!(log[0].hash, head.as_str());
        let branches =
            parse_branches(&fixture.run_at(&repo, &branches_plan()), Some(&head)).unwrap();
        assert_eq!(branches[0].commit_hash, head.as_str());
        let files =
            parse_commit_files(&fixture.run_at(&repo, &commit_files_plan(&head, None))).unwrap();
        assert_eq!(files[0].path, path);
        let tree = parse_tree_entry(
            &fixture.run_at(&repo, &tree_entry_plan(&head, path).unwrap()),
            path,
        )
        .unwrap()
        .unwrap();
        let index = parse_index_entry(
            &fixture.run_at(&repo, &index_entry_plan(path).unwrap()),
            path,
        )
        .unwrap()
        .unwrap();
        assert_eq!(tree.oid, index.oid);
        assert_eq!(tree.oid.as_str().len(), 64);
        assert_eq!(
            parse_blob_size(&fixture.run_at(&repo, &blob_size_plan(&tree.oid))).unwrap(),
            7
        );
        assert_eq!(fixture.run_at(&repo, &blob_plan(&tree.oid)), b"sha256\n");
    }

    #[test]
    fn actual_partial_status_diff_discard_and_index_lock_match_safety_contract() {
        let fixture = Fixture::new();
        fixture.file("tracked", b"HEAD\n");
        fixture.file("sibling", b"safe\n");
        let head = fixture.commit("root");
        fixture.file("tracked", b"INDEX\n");
        fixture.run(&stage_plan(&["tracked".to_string()]).unwrap());
        fixture.file("tracked", b"WORKTREE\n");
        let status = fixture.status();
        assert_eq!(status.changes[0].staged_status, Some(GitStatus::Modified));
        assert_eq!(status.changes[0].unstaged_status, Some(GitStatus::Modified));
        let local = crate::git::get_changes_status(&fixture.repo).unwrap();
        assert_eq!(
            serde_json::to_value(status.changes).unwrap(),
            serde_json::to_value(local).unwrap()
        );
        for staged in [false, true] {
            let actual =
                fixture.diff(&working_diff_plan(Some(&head), "tracked", None, staged).unwrap());
            let local = crate::git::get_git_diff(&fixture.repo, "tracked", Some(staged)).unwrap();
            assert_eq!(actual.old_content, "HEAD\n");
            assert_eq!(
                actual.new_content,
                if staged { "INDEX\n" } else { "WORKTREE\n" }
            );
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(local).unwrap()
            );
        }
        fixture.run(&unstage_plan(Some(&head), &["tracked".to_string()]).unwrap());
        assert!(fixture.status().changes[0].staged_status.is_none());
        assert_eq!(
            std::fs::read(fixture.repo.join("tracked")).unwrap(),
            b"WORKTREE\n"
        );
        fixture.run(&stage_plan(&["tracked".to_string()]).unwrap());
        let DiscardPlan::Git(discard) = discard_plan("tracked", Some(&head), true).unwrap() else {
            panic!()
        };
        fixture.run(&discard);
        assert!(fixture.status().changes.is_empty());
        assert_eq!(
            std::fs::read(fixture.repo.join("tracked")).unwrap(),
            b"HEAD\n"
        );
        assert_eq!(
            std::fs::read(fixture.repo.join("sibling")).unwrap(),
            b"safe\n"
        );
        std::fs::remove_file(fixture.repo.join("tracked")).unwrap();
        let diff = fixture.diff(&working_diff_plan(Some(&head), "tracked", None, false).unwrap());
        assert!(diff.new_content.is_empty());
        fixture.run(&stage_plan(&["tracked".to_string()]).unwrap());
        assert_eq!(
            fixture.status().changes[0].staged_status,
            Some(GitStatus::Deleted)
        );
        fixture.run(&unstage_all_plan(Some(&head)));
        assert_eq!(fixture.status().head.oid, Some(head));
        fixture.file("tracked", b"changed\n");
        fixture.file(".gitignore", b"ignored\n");
        std::fs::write(fixture.repo.join(".git/index.lock"), b"external lock").unwrap();
        let output = fixture.output_at(
            &fixture.repo,
            &stage_plan(&["tracked".to_string()]).unwrap().args,
        );
        assert!(!output.status.success());
        assert_eq!(
            std::fs::read(fixture.repo.join(".git/index.lock")).unwrap(),
            b"external lock"
        );
    }

    #[test]
    fn actual_history_filters_without_checkout_and_continues_from_both_merge_parents() {
        let fixture = Fixture::new();
        fixture.file("root", b"root\n");
        let root = fixture.commit("root subject\n\nroot body\n");
        fixture.raw(&["checkout", "-b", "side"]);
        fixture.file("side", b"side\n");
        let side = fixture.commit("side");
        fixture.raw(&["checkout", "main"]);
        fixture.file("main", b"main\n");
        let main = fixture.commit("main");
        fixture.raw(&["merge", "--no-ff", "-m", "merge", "side"]);
        let merge = fixture.status().head.oid.unwrap();
        let parents =
            parse_commit_parents(&fixture.run(&commit_parents_plan(&merge)), &merge).unwrap();
        assert_eq!(parents, [main.clone(), side.clone()]);
        let first =
            parse_log(&fixture.run(&log_plan(std::slice::from_ref(&merge), 1).unwrap().unwrap()))
                .unwrap();
        assert_eq!(first[0].hash, merge.as_str());
        assert_eq!(first[0].parent_hashes, [main.as_str(), side.as_str()]);
        let continuation =
            parse_log(&fixture.run(&log_plan(&parents, 30).unwrap().unwrap())).unwrap();
        let hashes: std::collections::HashSet<_> = continuation
            .iter()
            .map(|commit| commit.hash.as_str())
            .collect();
        assert_eq!(
            hashes,
            [main.as_str(), side.as_str(), root.as_str()]
                .into_iter()
                .collect()
        );
        let refs = parse_branches(&fixture.run(&branches_plan()), Some(&merge)).unwrap();
        let side_ref =
            GitRef::from_branch(refs.iter().find(|branch| branch.name == "side").unwrap()).unwrap();
        let side_tip = parse_object_id(&fixture.run(&resolve_ref_plan(&side_ref))).unwrap();
        let filtered =
            parse_log(&fixture.run(&log_plan(&[side_tip], 30).unwrap().unwrap())).unwrap();
        assert_eq!(filtered.len(), 2);
        let local = crate::git::get_git_log(&fixture.repo, None, Some(30), Some("side")).unwrap();
        assert_eq!(
            serde_json::to_value(&filtered).unwrap(),
            serde_json::to_value(local).unwrap()
        );
        assert_eq!(fixture.status().head.oid, Some(merge.clone()));
        let files =
            parse_commit_files(&fixture.run(&commit_files_plan(&merge, parents.first()))).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "side");
        let root_log = filtered
            .iter()
            .find(|commit| commit.hash == root.as_str())
            .unwrap();
        assert_eq!(root_log.body.as_deref(), Some("root body\n"));
        fixture.raw(&["checkout", "--detach", root.as_str()]);
        assert!(fixture.status().head.branch.is_none());
    }

    #[test]
    fn actual_clock_skewed_history_keeps_date_order_and_all_parent_continuations() {
        let fixture = Fixture::new();
        fixture.file("root", b"root\n");
        let initial = fixture.commit("initial");
        let tree = parse_object_id(
            &fixture.raw(&["rev-parse", &format!("{}^{{tree}}", initial.as_str())]),
        )
        .unwrap();
        let commit_at = |message: &str, date: &str, parents: &[&ObjectId]| {
            let mut args = vec![
                "commit-tree".to_string(),
                tree.as_str().to_string(),
                "-m".to_string(),
                message.to_string(),
            ];
            for parent in parents {
                args.extend(["-p".to_string(), parent.as_str().to_string()]);
            }
            let output = fixture.output_at_with_date(&fixture.repo, &args, date);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            parse_object_id(&output.stdout).unwrap()
        };
        let root = commit_at("future root", "2030-01-01T00:00:00Z", &[]);
        let left = commit_at("left", "2002-01-01T00:00:00Z", &[&root]);
        let right = commit_at("right", "2003-01-01T00:00:00Z", &[&root]);
        let merge = commit_at("old merge", "2000-01-01T00:00:00Z", &[&left, &right]);
        fixture.raw(&["update-ref", "refs/heads/main", merge.as_str()]);
        let log =
            parse_log(&fixture.run(&log_plan(std::slice::from_ref(&merge), 30).unwrap().unwrap()))
                .unwrap();
        let hashes: Vec<_> = log.iter().map(|commit| commit.hash.as_str()).collect();
        assert_eq!(
            hashes,
            [merge.as_str(), right.as_str(), left.as_str(), root.as_str()]
        );
        let local = crate::git::get_git_log(&fixture.repo, None, Some(30), None).unwrap();
        assert_eq!(
            serde_json::to_value(&log).unwrap(),
            serde_json::to_value(local).unwrap()
        );
        let first = parse_log(&fixture.run(&log_plan(&[merge], 1).unwrap().unwrap())).unwrap();
        let tips: Vec<_> = first[0]
            .parent_hashes
            .iter()
            .map(|hash| oid(hash))
            .collect();
        let continuation = parse_log(&fixture.run(&log_plan(&tips, 30).unwrap().unwrap())).unwrap();
        let continued: Vec<_> = continuation
            .iter()
            .map(|commit| commit.hash.as_str())
            .collect();
        assert_eq!(continued, [right.as_str(), left.as_str(), root.as_str()]);
    }

    #[test]
    fn actual_rename_binary_oversized_and_conflicted_entries_are_explicit() {
        let fixture = Fixture::new();
        fixture.file("old name", b"rename contents\n");
        fixture.file("binary", b"binary\0data");
        fixture.file("oversized", &vec![b'x'; MAX_BLOB_BYTES + 1]);
        fixture.file("conflict", b"base\n");
        let root = fixture.commit("root");
        std::fs::rename(
            fixture.repo.join("old name"),
            fixture.repo.join("new\nname"),
        )
        .unwrap();
        fixture.run(&stage_all_plan());
        let rename = fixture
            .status()
            .changes
            .into_iter()
            .find(|entry| entry.path == "new\nname")
            .unwrap();
        assert_eq!(rename.old_path.as_deref(), Some("old name"));
        fixture.run(&commit_plan("rename").unwrap());
        let head = fixture.status().head.oid.unwrap();
        let files =
            parse_commit_files(&fixture.run(&commit_files_plan(&head, Some(&root)))).unwrap();
        assert_eq!(files[0].old_path.as_deref(), Some("old name"));
        let diff = fixture
            .diff(&commit_diff_plan(&head, Some(&root), "new\nname", Some("old name")).unwrap());
        let local = crate::git::get_commit_file_diff(
            &fixture.repo,
            head.as_str(),
            "new\nname",
            Some("old name"),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(diff).unwrap(),
            serde_json::to_value(local).unwrap()
        );
        assert!(
            fixture
                .diff(&commit_diff_plan(&root, None, "binary", None).unwrap())
                .is_binary
        );
        let large = parse_tree_entry(
            &fixture.run(&tree_entry_plan(&root, "oversized").unwrap()),
            "oversized",
        )
        .unwrap()
        .unwrap();
        assert_eq!(large.size, Some(MAX_BLOB_BYTES as u64 + 1));
        assert!(build_diff(DiffContent::TooLarge, DiffContent::Missing).too_large);
        fixture.raw(&["checkout", "-b", "conflicting-side"]);
        fixture.file("conflict", b"side\n");
        fixture.commit("side conflict");
        fixture.raw(&["checkout", "main"]);
        fixture.file("conflict", b"main\n");
        fixture.commit("main conflict");
        let merge = fixture.output_at(
            &fixture.repo,
            &[
                "merge".to_string(),
                "--no-ff".to_string(),
                "conflicting-side".to_string(),
            ],
        );
        assert!(!merge.status.success());
        let status = fixture.status();
        let conflict = status
            .changes
            .iter()
            .find(|entry| entry.path == "conflict")
            .unwrap();
        assert_eq!(conflict.staged_status, Some(GitStatus::Conflicted));
        assert_eq!(conflict.unstaged_status, Some(GitStatus::Conflicted));
        assert!(
            parse_index_entry(
                &fixture.run(&index_entry_plan("conflict").unwrap()),
                "conflict"
            )
            .is_err()
        );
        assert!(fixture.repo.join(".git/MERGE_HEAD").exists());
        fixture.run(&unstage_all_plan(status.head.oid.as_ref()));
        assert!(!fixture.repo.join(".git/MERGE_HEAD").exists());
    }

    #[test]
    fn actual_worktree_authority_create_remove_force_and_prune_plans() {
        let fixture = Fixture::new();
        fixture.file("tracked", b"main sentinel\n");
        let head = fixture.commit("root");
        let plans = repository_authority_plans();
        let authority = parse_repository_authority(
            &fixture.run(&plans[0]),
            &fixture.run(&plans[1]),
            &fixture.run(&plans[2]),
        )
        .unwrap();
        assert!(!authority.is_linked_worktree());
        let target = fixture.root.join("linked\nworktree");
        let target_str = target.to_str().unwrap();
        fixture.run(
            &worktree_add_plan(
                target_str,
                &GitRef::local("feature").unwrap(),
                true,
                Some(&head),
            )
            .unwrap(),
        );
        let linked = parse_repository_authority(
            &fixture.run_at(&target, &plans[0]),
            &fixture.run_at(&target, &plans[1]),
            &fixture.run_at(&target, &plans[2]),
        )
        .unwrap();
        assert!(linked.is_linked_worktree());
        assert_eq!(linked.common_dir, authority.common_dir);
        let inventory = parse_worktrees(&fixture.run(&worktree_list_plan())).unwrap();
        assert_eq!(inventory.len(), 2);
        std::fs::write(target.join("tracked"), b"dirty\n").unwrap();
        let remove = worktree_remove_plan(target_str, &authority.worktree_root, false).unwrap();
        assert!(
            !fixture
                .output_at(&fixture.repo, &remove.args)
                .status
                .success()
        );
        assert!(target.exists());
        fixture.run(&worktree_remove_plan(target_str, &authority.worktree_root, true).unwrap());
        assert!(!target.exists());
        assert_eq!(
            std::fs::read(fixture.repo.join("tracked")).unwrap(),
            b"main sentinel\n"
        );
        fixture.run(
            &worktree_add_plan(target_str, &GitRef::local("feature").unwrap(), false, None)
                .unwrap(),
        );
        std::fs::remove_dir_all(&target).unwrap();
        fixture.run(&worktree_prune_plan());
        assert_eq!(
            parse_worktrees(&fixture.run(&worktree_list_plan()))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn actual_pull_push_plans_use_fixture_host_git_configuration() {
        let fixture = Fixture::new();
        fixture.file("tracked", b"initial\n");
        fixture.commit("initial");
        let origin = fixture.root.join("origin.git");
        let origin = origin.to_str().unwrap();
        fixture.raw(&["init", "--bare", "--initial-branch=main", origin]);
        fixture.raw(&["remote", "add", "origin", origin]);
        fixture.raw(&["push", "--set-upstream", "origin", "main"]);
        fixture.file("tracked", b"pushed\n");
        let pushed = fixture.commit("push via plan");
        fixture.run(&push_plan());
        assert_eq!(
            parse_object_id(&fixture.raw(&["--git-dir", origin, "rev-parse", "refs/heads/main"]))
                .unwrap(),
            pushed
        );
        let peer = fixture.root.join("peer");
        fixture.raw(&["clone", origin, peer.to_str().unwrap()]);
        std::fs::write(peer.join("tracked"), b"pulled\n").unwrap();
        fixture.run_at(&peer, &stage_all_plan());
        fixture.run_at(&peer, &commit_plan("peer update").unwrap());
        fixture.run_at(&peer, &push_plan());
        fixture.run(&pull_plan());
        assert_eq!(
            std::fs::read(fixture.repo.join("tracked")).unwrap(),
            b"pulled\n"
        );
        assert!(fixture.status().changes.is_empty());
    }

    #[test]
    fn actual_non_utf8_paths_and_symlink_blobs_do_not_alias_files() {
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let invalid = fixture
            .repo
            .join(std::ffi::OsString::from_vec(b"non-utf8-\xff".to_vec()));
        std::fs::write(&invalid, b"sentinel\n").unwrap();
        assert!(parse_status(&fixture.run(&status_plan())).is_err());
        std::fs::remove_file(&invalid).unwrap();
        std::fs::write(fixture.root.join("sentinel"), b"do not follow\n").unwrap();
        symlink("../sentinel", fixture.repo.join("link")).unwrap();
        let head = fixture.commit("symlink");
        let diff = fixture.diff(&commit_diff_plan(&head, None, "link", None).unwrap());
        assert_eq!(diff.new_content, "../sentinel");
        assert_eq!(
            std::fs::read(fixture.root.join("sentinel")).unwrap(),
            b"do not follow\n"
        );
    }
}
