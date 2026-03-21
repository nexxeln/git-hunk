use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use assert_cmd::cargo::CommandCargoExt;
use serde_json::Value;
use tempfile::TempDir;

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

fn first_change_id(scan: &Value) -> String {
    scan["files"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|file| file["hunks"].as_array().unwrap().iter())
        .flat_map(|hunk| hunk["changes"].as_array().unwrap().iter())
        .next()
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
