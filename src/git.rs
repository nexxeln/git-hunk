use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

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

pub fn repo_root(cwd: &Path) -> AppResult<PathBuf> {
    let output = run_git(cwd, ["rev-parse", "--show-toplevel"], &[0])?;
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
            )? {
                changed.insert(path);
            }
            for path in list_z(
                repo_root,
                ["ls-files", "--others", "--exclude-standard", "-z"],
            )? {
                untracked.insert(path.clone());
                changed.insert(path);
            }
        }
        Mode::Unstage => {
            for path in list_z(
                repo_root,
                ["diff", "--cached", "--name-only", "-z", "--diff-filter=ADM"],
            )? {
                changed.insert(path);
            }
        }
    }

    let conflicted = match mode {
        Mode::Stage => list_z(repo_root, ["diff", "--name-only", "-z", "--diff-filter=U"]),
        Mode::Unstage => list_z(
            repo_root,
            ["diff", "--cached", "--name-only", "-z", "--diff-filter=U"],
        ),
    }?
    .into_iter()
    .collect();

    let unsupported = match mode {
        Mode::Stage => list_z(repo_root, ["diff", "--name-only", "-z", "--diff-filter=RC"]),
        Mode::Unstage => list_z(
            repo_root,
            ["diff", "--cached", "--name-only", "-z", "--diff-filter=RC"],
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
        run_git(repo_root, args, &[0])?
    };

    String::from_utf8(output.stdout).map_err(|_| {
        AppError::new(
            "non_utf8_diff",
            format!("diff for '{}' is not valid utf-8", path),
        )
    })
}

pub fn apply_patch(repo_root: &Path, patch: &str, reverse: bool) -> AppResult<()> {
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
    run_git(repo_root, check_args, &[0])?;

    base_args.push(path.as_str());
    run_git(repo_root, base_args, &[0])?;
    Ok(())
}

pub fn has_staged_changes(repo_root: &Path) -> AppResult<bool> {
    let output = run_git(
        repo_root,
        ["diff", "--cached", "--quiet", "--exit-code"],
        &[0, 1],
    )?;
    Ok(output.status == 1)
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
    run_git(repo_root, args, &[0])?;

    let output = run_git(repo_root, ["rev-parse", "HEAD"], &[0])?;
    let sha = String::from_utf8(output.stdout)
        .map_err(|_| AppError::new("non_utf8_sha", "HEAD sha is not valid utf-8".to_string()))?;
    Ok(sha.trim().to_string())
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

fn list_z<I, S>(repo_root: &Path, args: I) -> AppResult<Vec<String>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = run_git(repo_root, args, &[0])?;
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

fn run_git<I, S>(repo_root: &Path, args: I, ok_codes: &[i32]) -> AppResult<GitOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new("git");
    command.current_dir(repo_root);
    command.arg("-c").arg("color.ui=false");
    command.arg("-c").arg("core.pager=cat");

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
            "git_failed",
            format!(
                "git {} failed with exit {}: {}",
                debug_args.join(" "),
                code,
                stderr
            ),
        ));
    }

    Ok(GitOutput {
        stdout: output.stdout,
        status: code,
    })
}
