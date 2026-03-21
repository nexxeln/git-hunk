use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::cargo::CommandCargoExt;
use serde_json::Value;
use tempfile::{NamedTempFile, TempDir};

#[test]
fn stage_change_only_updates_selected_block() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let change_id = first_change_id(&scan);

    let _stage = cli_json(
        repo.path(),
        &[
            "stage",
            "--snapshot",
            snapshot,
            "--change",
            &change_id,
            "--json",
        ],
    );

    let staged = git_stdout(repo.path(), &["diff", "--cached", "--", "note.txt"]);
    assert!(staged.contains("beta-1"));
    assert!(!staged.contains("iota-1"));

    let unstaged = git_stdout(repo.path(), &["diff", "--", "note.txt"]);
    assert!(!unstaged.contains("beta-1"));
    assert!(unstaged.contains("iota-1"));
}

#[test]
fn stage_line_range_selects_single_change() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let hunk_id = first_hunk_id_for_path(&scan, "note.txt");
    let selector = format!("{}:new:2-2", hunk_id);

    let _stage = cli_json(
        repo.path(),
        &[
            "stage",
            "--snapshot",
            snapshot,
            "--hunk",
            &selector,
            "--json",
        ],
    );

    let staged = git_stdout(repo.path(), &["diff", "--cached", "--", "note.txt"]);
    assert!(staged.contains("beta-1"));
    assert!(!staged.contains("iota-1"));
}

#[test]
fn resolve_exact_change_prefers_change_id() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let first_change = nth_change_id(&scan, 0);

    let resolved = cli_json(
        repo.path(),
        &[
            "resolve",
            "--mode",
            "stage",
            "--snapshot",
            snapshot,
            "--path",
            "note.txt",
            "--start",
            "2",
            "--json",
        ],
    );

    assert_eq!(resolved["status"], "exact");
    assert_eq!(resolved["matched_side"], "new");
    assert_eq!(resolved["recommended_change_ids"][0], first_change);
    assert_eq!(
        resolved["recommended_hunk_selectors"][0],
        format!("{}:new:2-2", first_hunk_id_for_path(&scan, "note.txt"))
    );
}

#[test]
fn resolve_partial_hint_expands_to_full_change() {
    let repo = init_repo();
    write_file(repo.path(), "pair.txt", "one\ntwo\nthree\nfour\n");
    git(repo.path(), &["add", "pair.txt"]);
    git(repo.path(), &["commit", "-m", "pair seed"]);

    write_file(repo.path(), "pair.txt", "one\nTWO\nTHREE\nfour\n");

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let change_id = nth_change_id(&scan, 0);

    let resolved = cli_json(
        repo.path(),
        &[
            "resolve",
            "--mode",
            "stage",
            "--snapshot",
            snapshot,
            "--path",
            "pair.txt",
            "--start",
            "2",
            "--end",
            "2",
            "--json",
        ],
    );

    assert_eq!(resolved["status"], "adjusted");
    assert_eq!(resolved["recommended_change_ids"][0], change_id);
    assert_eq!(
        resolved["recommended_hunk_selectors"][0],
        format!("{}:new:2-3", first_hunk_id_for_path(&scan, "pair.txt"))
    );
}

#[test]
fn resolve_can_recommend_multiple_change_ids() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();

    let resolved = cli_json(
        repo.path(),
        &[
            "resolve",
            "--mode",
            "stage",
            "--snapshot",
            snapshot,
            "--path",
            "note.txt",
            "--start",
            "2",
            "--end",
            "9",
            "--json",
        ],
    );

    assert_eq!(resolved["status"], "exact");
    assert_eq!(
        resolved["recommended_change_ids"].as_array().unwrap().len(),
        2
    );
}

#[test]
fn resolve_nearest_change_when_no_overlap_exists() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let second_change = nth_change_id(&scan, 1);

    let resolved = cli_json(
        repo.path(),
        &[
            "resolve",
            "--mode",
            "stage",
            "--snapshot",
            snapshot,
            "--path",
            "note.txt",
            "--start",
            "8",
            "--end",
            "8",
            "--json",
        ],
    );

    assert_eq!(resolved["status"], "nearest");
    assert_eq!(resolved["recommended_change_ids"][0], second_change);
}

#[test]
fn resolve_auto_uses_old_side_for_deletions() {
    let repo = init_repo();
    write_file(repo.path(), "note.txt", "alpha\nbeta\ngamma\n");
    git(repo.path(), &["add", "note.txt"]);
    git(repo.path(), &["commit", "-m", "seed delete"]);

    write_file(repo.path(), "note.txt", "alpha\ngamma\n");

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();

    let resolved = cli_json(
        repo.path(),
        &[
            "resolve",
            "--mode",
            "stage",
            "--snapshot",
            snapshot,
            "--path",
            "note.txt",
            "--start",
            "2",
            "--json",
        ],
    );

    assert_eq!(resolved["matched_side"], "old");
    assert_eq!(resolved["status"], "exact");
}

#[test]
fn resolve_rejects_unknown_path() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();

    let output = cli_output(
        repo.path(),
        &[
            "resolve",
            "--mode",
            "stage",
            "--snapshot",
            snapshot,
            "--path",
            "missing.txt",
            "--start",
            "1",
            "--json",
        ],
    );
    assert!(!output.status.success());

    let err: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(err["error"]["code"], "unknown_path");
    assert_eq!(err["error"]["category"], "selector");
}

#[test]
fn compact_scan_summarizes_changes_for_agents() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );

    let scan = cli_json(
        repo.path(),
        &["scan", "--mode", "stage", "--compact", "--json"],
    );
    let change = &scan["files"][0]["hunks"][0]["changes"][0];

    assert!(change.get("lines").is_none());
    assert_eq!(change["metadata"]["kind"], "replacement");
    assert_eq!(change["metadata"]["added_lines"], 1);
    assert_eq!(change["metadata"]["deleted_lines"], 1);
    assert!(
        change["metadata"]["preview"]
            .as_str()
            .unwrap()
            .contains("beta")
    );
}

#[test]
fn unstage_change_only_removes_selected_block() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );
    git(repo.path(), &["add", "note.txt"]);

    let scan = cli_json(repo.path(), &["scan", "--mode", "unstage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let change_id = first_change_id(&scan);

    let _unstage = cli_json(
        repo.path(),
        &[
            "unstage",
            "--snapshot",
            snapshot,
            "--change",
            &change_id,
            "--json",
        ],
    );

    let staged = git_stdout(repo.path(), &["diff", "--cached", "--", "note.txt"]);
    assert!(!staged.contains("beta-1"));
    assert!(staged.contains("iota-1"));

    let unstaged = git_stdout(repo.path(), &["diff", "--", "note.txt"]);
    assert!(unstaged.contains("beta-1"));
    assert!(!unstaged.contains("iota-1"));
}

#[test]
fn unstage_line_range_selects_single_change_on_old_side() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );
    git(repo.path(), &["add", "note.txt"]);

    let scan = cli_json(repo.path(), &["scan", "--mode", "unstage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let hunk_id = first_hunk_id_for_path(&scan, "note.txt");
    let selector = format!("{}:old:2-2", hunk_id);

    let _unstage = cli_json(
        repo.path(),
        &[
            "unstage",
            "--snapshot",
            snapshot,
            "--hunk",
            &selector,
            "--json",
        ],
    );

    let staged = git_stdout(repo.path(), &["diff", "--cached", "--", "note.txt"]);
    assert!(!staged.contains("beta-1"));
    assert!(staged.contains("iota-1"));
}

#[test]
fn stale_snapshot_is_rejected() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap().to_string();
    let change_id = first_change_id(&scan);

    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-2\nkappa\n",
    );

    let output = cli_output(
        repo.path(),
        &[
            "stage",
            "--snapshot",
            &snapshot,
            "--change",
            &change_id,
            "--json",
        ],
    );
    assert!(!output.status.success());

    let err: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(err["error"]["code"], "stale_snapshot");
    assert_eq!(err["error"]["category"], "snapshot");
    assert_eq!(err["error"]["retryable"], true);
}

#[test]
fn commit_stages_selection_before_writing_commit() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let change_id = first_change_id(&scan);

    let commit = cli_json(
        repo.path(),
        &[
            "commit",
            "-m",
            "pick first block",
            "--snapshot",
            snapshot,
            "--change",
            &change_id,
            "--json",
        ],
    );
    assert!(commit["commit"].as_str().unwrap().len() >= 7);

    let log = git_stdout(repo.path(), &["log", "-1", "--pretty=%s"]);
    assert_eq!(log.trim(), "pick first block");

    let commit_diff = git_stdout(repo.path(), &["diff", "HEAD~1..HEAD", "--", "note.txt"]);
    assert!(commit_diff.contains("beta-1"));
    assert!(!commit_diff.contains("iota-1"));

    let remaining = git_stdout(repo.path(), &["diff", "--", "note.txt"]);
    assert!(!remaining.contains("beta-1"));
    assert!(remaining.contains("iota-1"));
}

#[test]
fn commit_line_range_stages_selection_before_writing_commit() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let hunk_id = first_hunk_id_for_path(&scan, "note.txt");
    let selector = format!("{}:new:9-9", hunk_id);

    let commit = cli_json(
        repo.path(),
        &[
            "commit",
            "-m",
            "pick ranged block",
            "--snapshot",
            snapshot,
            "--hunk",
            &selector,
            "--json",
        ],
    );
    assert!(commit["commit"].as_str().unwrap().len() >= 7);
    assert_eq!(commit["selected_line_ranges"][0], selector);

    let commit_diff = git_stdout(repo.path(), &["diff", "HEAD~1..HEAD", "--", "note.txt"]);
    assert!(!commit_diff.contains("beta-1"));
    assert!(commit_diff.contains("iota-1"));
}

#[test]
fn commit_dry_run_returns_exact_patch_without_mutating_repo() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let change_id = first_change_id(&scan);
    let head_before = git_stdout(repo.path(), &["rev-parse", "HEAD"]);

    let dry_run = cli_json(
        repo.path(),
        &[
            "commit",
            "-m",
            "preview selection",
            "--snapshot",
            snapshot,
            "--change",
            &change_id,
            "--dry-run",
            "--json",
        ],
    );

    assert_eq!(dry_run["dry_run"], true);
    assert_eq!(dry_run["files"][0], "note.txt");
    assert!(dry_run["patch"].as_str().unwrap().contains("beta-1"));
    assert!(!dry_run["patch"].as_str().unwrap().contains("iota-1"));
    assert!(dry_run["diffstat"].as_str().unwrap().contains("note.txt"));

    let head_after = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    assert_eq!(head_before, head_after);

    let staged = git_stdout(repo.path(), &["diff", "--cached"]);
    assert!(staged.trim().is_empty());

    let unstaged = git_stdout(repo.path(), &["diff", "--", "note.txt"]);
    assert!(unstaged.contains("beta-1"));
    assert!(unstaged.contains("iota-1"));
}

#[test]
fn commit_dry_run_includes_already_staged_changes() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(repo.path(), "staged.txt", "before\nafter\n");
    git(repo.path(), &["add", "staged.txt"]);
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let change_id = first_change_id(&scan);

    let dry_run = cli_json(
        repo.path(),
        &[
            "commit",
            "-m",
            "preview staged and selected",
            "--snapshot",
            snapshot,
            "--change",
            &change_id,
            "--dry-run",
            "--json",
        ],
    );

    let files = dry_run["files"].as_array().unwrap();
    assert!(files.iter().any(|file| file == "note.txt"));
    assert!(files.iter().any(|file| file == "staged.txt"));
    let patch = dry_run["patch"].as_str().unwrap();
    assert!(patch.contains("staged.txt"));
    assert!(patch.contains("beta-1"));
}

#[test]
fn line_range_rejects_partial_change_overlap() {
    let repo = init_repo();
    write_file(repo.path(), "pair.txt", "one\ntwo\nthree\nfour\n");
    git(repo.path(), &["add", "pair.txt"]);
    git(repo.path(), &["commit", "-m", "pair seed"]);

    write_file(repo.path(), "pair.txt", "one\nTWO\nTHREE\nfour\n");

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let hunk_id = first_hunk_id_for_path(&scan, "pair.txt");
    let selector = format!("{}:new:2-2", hunk_id);

    let output = cli_output(
        repo.path(),
        &[
            "stage",
            "--snapshot",
            snapshot,
            "--hunk",
            &selector,
            "--json",
        ],
    );
    assert!(!output.status.success());

    let err: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(err["error"]["code"], "ambiguous_line_range");
    assert_eq!(err["error"]["category"], "selector");
    assert_eq!(err["error"]["retryable"], false);
}

#[test]
fn plan_file_can_select_line_range() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    write_file(
        repo.path(),
        "note.txt",
        "alpha\nbeta-1\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota-1\nkappa\n",
    );

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let hunk_id = first_hunk_id_for_path(&scan, "note.txt");
    let plan_file = NamedTempFile::new().unwrap();
    let plan_path = plan_file.path();
    fs::write(
        plan_path,
        format!(
            "{{\n  \"snapshot_id\": \"{}\",\n  \"selectors\": [\n    {{\n      \"type\": \"line_range\",\n      \"hunk_id\": \"{}\",\n      \"side\": \"new\",\n      \"start\": 2,\n      \"end\": 2\n    }}\n  ]\n}}\n",
            snapshot, hunk_id
        ),
    )
    .unwrap();

    let _stage = cli_json(
        repo.path(),
        &["stage", "--plan", plan_path.to_str().unwrap(), "--json"],
    );

    let staged = git_stdout(repo.path(), &["diff", "--cached", "--", "note.txt"]);
    assert!(staged.contains("beta-1"));
    assert!(!staged.contains("iota-1"));
}

#[test]
fn unborn_head_can_commit_new_file_from_selection() {
    let repo = init_repo();
    write_file(repo.path(), "hello.txt", "hello\nworld\n");

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let snapshot = scan["snapshot_id"].as_str().unwrap();
    let hunk_id = first_hunk_id(&scan);

    let commit = cli_json(
        repo.path(),
        &[
            "commit",
            "-m",
            "initial import",
            "--snapshot",
            snapshot,
            "--hunk",
            &hunk_id,
            "--json",
        ],
    );
    assert!(commit["commit"].as_str().unwrap().len() >= 7);

    let count = git_stdout(repo.path(), &["rev-list", "--count", "HEAD"]);
    assert_eq!(count.trim(), "1");

    let status = git_stdout(repo.path(), &["status", "--short"]);
    assert!(status.trim().is_empty());
}

#[test]
fn tracked_non_utf8_diff_is_reported_as_unsupported() {
    let repo = init_repo();
    write_bytes(repo.path(), "blob.bin", &[0xff, 0xfe, 0xfd]);
    git(repo.path(), &["add", "blob.bin"]);
    git(repo.path(), &["commit", "-m", "binary seed"]);

    write_bytes(repo.path(), "blob.bin", &[0xff, 0xfe, 0xfc]);

    let scan = cli_json(repo.path(), &["scan", "--mode", "stage", "--json"]);
    let unsupported = scan["unsupported"].as_array().unwrap();
    assert!(unsupported.iter().any(|item| item["path"] == "blob.bin"));
}

#[test]
fn rename_is_reported_as_unsupported_in_unstage_mode() {
    let repo = init_repo();
    seed_committed_file(repo.path());
    git(repo.path(), &["mv", "note.txt", "renamed.txt"]);

    let scan = cli_json(repo.path(), &["scan", "--mode", "unstage", "--json"]);
    let unsupported = scan["unsupported"].as_array().unwrap();
    assert!(unsupported.iter().any(|item| item["path"] == "renamed.txt"));
}

fn init_repo() -> TempDir {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "--initial-branch=main"]);
    repo
}

fn seed_committed_file(repo: &Path) {
    write_file(
        repo,
        "note.txt",
        "alpha\nbeta\ngamma\ndelta\nepsilon\nzeta\neta\ntheta\niota\nkappa\n",
    );
    git(repo, &["add", "note.txt"]);
    git(repo, &["commit", "-m", "initial"]);
}

fn write_file(repo: &Path, path: &str, contents: &str) {
    let full = repo.join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(full, contents).unwrap();
}

fn write_bytes(repo: &Path, path: &str, contents: &[u8]) {
    let full = repo.join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(full, contents).unwrap();
}

fn first_hunk_id(scan: &Value) -> String {
    scan["files"][0]["hunks"][0]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn first_hunk_id_for_path(scan: &Value, path: &str) -> String {
    scan["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == path)
        .and_then(|file| file["hunks"][0]["id"].as_str())
        .unwrap()
        .to_string()
}

fn first_change_id(scan: &Value) -> String {
    nth_change_id(scan, 0)
}

fn nth_change_id(scan: &Value, index: usize) -> String {
    scan["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|file| file["hunks"].as_array().unwrap().iter())
        .flat_map(|hunk| hunk["changes"].as_array().unwrap().iter())
        .nth(index)
        .and_then(|change| change["id"].as_str())
        .unwrap()
        .to_string()
}

fn cli_json(repo: &Path, args: &[&str]) -> Value {
    let output = cli_output(repo, args);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn cli_output(repo: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::cargo_bin("git-hunk").unwrap();
    cmd.current_dir(repo);
    cmd.args(args);
    cmd.env("GIT_AUTHOR_NAME", "Test User");
    cmd.env("GIT_AUTHOR_EMAIL", "test@example.com");
    cmd.env("GIT_COMMITTER_NAME", "Test User");
    cmd.env("GIT_COMMITTER_EMAIL", "test@example.com");
    cmd.output().unwrap()
}

fn git(repo: &Path, args: &[&str]) {
    let output = git_output(repo, args);
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let output = git_output(repo, args);
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn git_output(repo: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo);
    cmd.args(args);
    cmd.env("GIT_AUTHOR_NAME", "Test User");
    cmd.env("GIT_AUTHOR_EMAIL", "test@example.com");
    cmd.env("GIT_COMMITTER_NAME", "Test User");
    cmd.env("GIT_COMMITTER_EMAIL", "test@example.com");
    cmd.output().unwrap()
}
