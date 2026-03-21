use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;
use tempfile::NamedTempFile;

use crate::cli::Mode;
use crate::error::{AppError, AppResult};
use crate::model::UnsupportedPath;

#[derive(Debug)]
pub struct PathInventory {
    pub changed: Vec<String>,
    pub untracked: BTreeSet<String>,
    pub conflicted: BTreeSet<String>,
    pub unsupported: Vec<UnsupportedPath>,
}

#[derive(Debug)]
pub struct CommitPreview {
    pub patch: String,
    pub files: Vec<String>,
    pub diffstat: String,
}

pub fn repo_root(cwd: &Path) -> AppResult<PathBuf> {
    let output = run_git(
        cwd,
        ["rev-parse", "--show-toplevel"],
        &[0],
        "git_repo_root_failed",
        None,
    )?;
    let root = String::from_utf8(output.stdout).map_err(|_| {
        AppError::new(
            "non_utf8_path",
            "repository path is not valid utf-8".to_string(),
        )
    })?;
    Ok(PathBuf::from(root.trim()))
}

pub fn inventory(repo_root: &Path, mode: Mode) -> AppResult<PathInventory> {
    let mut changed = BTreeSet::new();
    let mut untracked = BTreeSet::new();

    match mode {
        Mode::Stage => {
            for path in list_z(
                repo_root,
                ["diff", "--name-only", "-z", "--diff-filter=ADM"],
                "git_inventory_failed",
                None,
            )? {
                changed.insert(path);
            }
            for path in list_z(
                repo_root,
                ["ls-files", "--others", "--exclude-standard", "-z"],
                "git_inventory_failed",
                None,
            )? {
                untracked.insert(path.clone());
                changed.insert(path);
            }
        }
        Mode::Unstage => {
            for path in list_z(
                repo_root,
                ["diff", "--cached", "--name-only", "-z", "--diff-filter=ADM"],
                "git_inventory_failed",
                None,
            )? {
                changed.insert(path);
            }
        }
    }

    let conflicted = match mode {
        Mode::Stage => list_z(
            repo_root,
            ["diff", "--name-only", "-z", "--diff-filter=U"],
            "git_inventory_failed",
            None,
        ),
        Mode::Unstage => list_z(
            repo_root,
            ["diff", "--cached", "--name-only", "-z", "--diff-filter=U"],
            "git_inventory_failed",
            None,
        ),
    }?
    .into_iter()
    .collect();

    let unsupported = match mode {
        Mode::Stage => list_z(
            repo_root,
            ["diff", "--name-only", "-z", "--diff-filter=RC"],
            "git_inventory_failed",
            None,
        ),
        Mode::Unstage => list_z(
            repo_root,
            ["diff", "--cached", "--name-only", "-z", "--diff-filter=RC"],
            "git_inventory_failed",
            None,
        ),
    }?
    .into_iter()
    .map(|path| UnsupportedPath {
        path,
        reason: "rename and copy changes are not supported".to_string(),
    })
    .collect();

    Ok(PathInventory {
        changed: changed.into_iter().collect(),
        untracked,
        conflicted,
        unsupported,
    })
}

pub fn diff_for_path(
    repo_root: &Path,
    mode: Mode,
    path: &str,
    unified: u32,
    untracked: bool,
) -> AppResult<String> {
    let unified = format!("--unified={}", unified);
    let output = if untracked {
        run_git(
            repo_root,
            [
                "diff",
                "--no-index",
                unified.as_str(),
                "--",
                "/dev/null",
                path,
            ],
            &[0, 1],
            "git_diff_failed",
            None,
        )?
    } else {
        let args = match mode {
            Mode::Stage => vec!["diff", "--no-ext-diff", unified.as_str(), "--", path],
            Mode::Unstage => {
                vec![
                    "diff",
                    "--cached",
                    "--no-ext-diff",
                    unified.as_str(),
                    "--",
                    path,
                ]
            }
        };
        run_git(repo_root, args, &[0], "git_diff_failed", None)?
    };

    String::from_utf8(output.stdout).map_err(|_| {
        AppError::new(
            "non_utf8_diff",
            format!("diff for '{}' is not valid utf-8", path),
        )
    })
}

pub fn apply_patch(repo_root: &Path, patch: &str, reverse: bool) -> AppResult<()> {
    apply_patch_with_index(repo_root, patch, reverse, None)
}

pub fn has_staged_changes(repo_root: &Path) -> AppResult<bool> {
    has_staged_changes_with_index(repo_root, None)
}

pub fn commit(repo_root: &Path, messages: &[String], allow_empty: bool) -> AppResult<String> {
    let mut args = vec!["commit"];
    if allow_empty {
        args.push("--allow-empty");
    }
    for message in messages {
        args.push("-m");
        args.push(message.as_str());
    }
    run_git(repo_root, args, &[0], "git_commit_failed", None)?;

    let output = run_git(
        repo_root,
        ["rev-parse", "HEAD"],
        &[0],
        "git_rev_parse_failed",
        None,
    )?;
    let sha = String::from_utf8(output.stdout)
        .map_err(|_| AppError::new("non_utf8_sha", "HEAD sha is not valid utf-8".to_string()))?;
    Ok(sha.trim().to_string())
}

pub fn preview_commit(
    repo_root: &Path,
    selection_patch: Option<&str>,
    allow_empty: bool,
) -> AppResult<CommitPreview> {
    let index = prepare_temp_index(repo_root)?;

    if let Some(patch) = selection_patch {
        apply_patch_with_index(repo_root, patch, false, Some(index.path()))?;
    }

    if !allow_empty && !has_staged_changes_with_index(repo_root, Some(index.path()))? {
        return Err(AppError::new(
            "nothing_staged",
            "there are no staged changes to commit".to_string(),
        ));
    }

    Ok(CommitPreview {
        patch: cached_diff(repo_root, Some(index.path()))?,
        files: cached_name_only(repo_root, Some(index.path()))?,
        diffstat: cached_diffstat(repo_root, Some(index.path()))?,
    })
}

pub fn read_file_text(repo_root: &Path, path: &str) -> AppResult<String> {
    let full_path = repo_root.join(path);
    let bytes = std::fs::read(&full_path).map_err(|err| {
        AppError::new(
            "file_read_failed",
            format!("failed to read {}: {}", full_path.display(), err),
        )
    })?;
    String::from_utf8(bytes)
        .map_err(|_| AppError::new("binary_file", format!("{} is not valid utf-8", path)))
}

fn apply_patch_with_index(
    repo_root: &Path,
    patch: &str,
    reverse: bool,
    index_file: Option<&Path>,
) -> AppResult<()> {
    let mut temp = NamedTempFile::new_in(repo_root).map_err(AppError::io)?;
    std::io::Write::write_all(&mut temp, patch.as_bytes()).map_err(AppError::io)?;

    let path = temp.path().to_string_lossy().to_string();

    let mut base_args = vec!["apply", "--cached", "--unidiff-zero"];
    if reverse {
        base_args.push("--reverse");
    }
    let mut check_args = base_args.clone();
    check_args.push("--check");
    check_args.push(path.as_str());
    run_git(
        repo_root,
        check_args,
        &[0],
        "git_apply_check_failed",
        index_file,
    )?;

    base_args.push(path.as_str());
    run_git(repo_root, base_args, &[0], "git_apply_failed", index_file)?;
    Ok(())
}

fn has_staged_changes_with_index(repo_root: &Path, index_file: Option<&Path>) -> AppResult<bool> {
    let output = run_git(
        repo_root,
        ["diff", "--cached", "--quiet", "--exit-code"],
        &[0, 1],
        "git_diff_check_failed",
        index_file,
    )?;
    Ok(output.status == 1)
}

fn cached_diff(repo_root: &Path, index_file: Option<&Path>) -> AppResult<String> {
    let output = run_git(
        repo_root,
        ["diff", "--cached", "--no-ext-diff"],
        &[0],
        "git_diff_failed",
        index_file,
    )?;
    String::from_utf8(output.stdout).map_err(|_| {
        AppError::new(
            "non_utf8_diff",
            "cached diff is not valid utf-8".to_string(),
        )
    })
}

fn cached_name_only(repo_root: &Path, index_file: Option<&Path>) -> AppResult<Vec<String>> {
    list_z(
        repo_root,
        ["diff", "--cached", "--name-only", "-z"],
        "git_diff_name_only_failed",
        index_file,
    )
}

fn cached_diffstat(repo_root: &Path, index_file: Option<&Path>) -> AppResult<String> {
    let output = run_git(
        repo_root,
        ["diff", "--cached", "--stat"],
        &[0],
        "git_diff_failed",
        index_file,
    )?;
    String::from_utf8(output.stdout).map_err(|_| {
        AppError::new(
            "non_utf8_diff",
            "cached diffstat is not valid utf-8".to_string(),
        )
    })
}

fn prepare_temp_index(repo_root: &Path) -> AppResult<NamedTempFile> {
    let temp = NamedTempFile::new().map_err(AppError::io)?;
    let index_path = git_index_path(repo_root)?;

    match std::fs::metadata(&index_path) {
        Ok(metadata) if metadata.len() > 0 => {
            std::fs::copy(&index_path, temp.path()).map_err(AppError::io)?;
        }
        Ok(_) => {
            run_git(
                repo_root,
                ["read-tree", "--empty"],
                &[0],
                "git_read_tree_failed",
                Some(temp.path()),
            )?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            run_git(
                repo_root,
                ["read-tree", "--empty"],
                &[0],
                "git_read_tree_failed",
                Some(temp.path()),
            )?;
        }
        Err(err) => return Err(AppError::io(err)),
    }

    Ok(temp)
}

fn git_index_path(repo_root: &Path) -> AppResult<PathBuf> {
    let output = run_git(
        repo_root,
        ["rev-parse", "--git-path", "index"],
        &[0],
        "git_index_path_failed",
        None,
    )?;
    let raw = String::from_utf8(output.stdout).map_err(|_| {
        AppError::new(
            "non_utf8_path",
            "git index path is not valid utf-8".to_string(),
        )
    })?;
    let raw = PathBuf::from(raw.trim());
    if raw.is_absolute() {
        Ok(raw)
    } else {
        Ok(repo_root.join(raw))
    }
}

fn list_z<I, S>(
    repo_root: &Path,
    args: I,
    error_code: &'static str,
    index_file: Option<&Path>,
) -> AppResult<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git(repo_root, args, &[0], error_code, index_file)?;
    let stdout = output.stdout;
    let items = stdout
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .map(|item| {
            String::from_utf8(item.to_vec()).map_err(|_| {
                AppError::new("non_utf8_path", "git returned a non-utf8 path".to_string())
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(items)
}

pub struct GitOutput {
    pub stdout: Vec<u8>,
    pub status: i32,
}

fn run_git<I, S>(
    repo_root: &Path,
    args: I,
    ok_codes: &[i32],
    error_code: &'static str,
    index_file: Option<&Path>,
) -> AppResult<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.current_dir(repo_root);
    command.arg("-c").arg("color.ui=false");
    command.arg("-c").arg("core.pager=cat");
    if let Some(index_file) = index_file {
        command.env("GIT_INDEX_FILE", index_file);
    }

    let debug_args = args
        .into_iter()
        .map(|arg| {
            let owned = arg.as_ref().to_owned();
            command.arg(&owned);
            owned.to_string_lossy().to_string()
        })
        .collect::<Vec<_>>();

    let output = command.output().map_err(AppError::io)?;
    let code = output.status.code().unwrap_or(1);
    if !ok_codes.contains(&code) {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::new(
            error_code,
            format!(
                "git {} failed with exit {}: {}",
                debug_args.join(" "),
                code,
                stderr
            ),
        )
        .with_details(json!({
            "command": debug_args,
            "exit_code": code,
            "stderr": stderr,
            "index_file": index_file.map(|path| path.display().to_string()),
        })));
    }

    Ok(GitOutput {
        stdout: output.stdout,
        status: code,
    })
}
