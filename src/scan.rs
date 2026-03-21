use crate::cli::Mode;
use crate::diff::{ParsedHunk, ParsedPatch, parse_patch};
use crate::error::{AppError, AppResult};
use crate::git;
use crate::model::{
    ChangeState, ChangeView, FileState, FileView, HunkState, HunkView, ScanState, SnapshotView,
    UnsupportedPath,
};

pub fn scan_repo(repo_root: &std::path::Path, mode: Mode) -> AppResult<ScanState> {
    let inventory = git::inventory(repo_root, mode)?;
    let mut files = Vec::new();
    let mut unsupported = inventory.unsupported;
    unsupported.extend(
        inventory
            .conflicted
            .into_iter()
            .map(|path| UnsupportedPath {
                path,
                reason: "conflicted path".to_string(),
            }),
    );

    for path in inventory.changed {
        if unsupported.iter().any(|item| item.path == path) {
            continue;
        }

        let untracked = inventory.untracked.contains(&path);
        match scan_path(repo_root, mode, &path, untracked) {
            Ok(file) => files.push(file),
            Err(err)
                if err.code == "binary_file"
                    || err.code == "unsupported_diff"
                    || err.code == "empty_diff"
                    || err.code == "non_utf8_diff" =>
            {
                unsupported.push(UnsupportedPath {
                    path,
                    reason: err.message,
                });
            }
            Err(err) => return Err(err),
        }
    }

    let snapshot_id = snapshot_id(mode, &files, &unsupported);

    for (file_index, file) in files.iter_mut().enumerate() {
        for (hunk_index, hunk) in file.hunks.iter_mut().enumerate() {
            hunk.id = short_id(
                "h",
                &format!(
                    "{}:{}:{}:{}:{}",
                    snapshot_id,
                    file.path,
                    hunk_index,
                    hunk.header,
                    render_raw_lines(&hunk.lines)
                ),
            );
            for change_index in &hunk.change_indexes {
                let change = &mut file.changes[*change_index];
                change.id = short_id(
                    "c",
                    &format!(
                        "{}:{}:{}:{}:{}:{}",
                        snapshot_id,
                        file.path,
                        file_index,
                        hunk_index,
                        change.header,
                        render_raw_lines(&change.lines)
                    ),
                );
            }
        }
    }

    let snapshot = SnapshotView {
        snapshot_id,
        mode,
        files: files
            .iter()
            .map(|file| FileView {
                path: file.path.clone(),
                status: file.status,
                hunks: file
                    .hunks
                    .iter()
                    .map(|hunk| HunkView {
                        id: hunk.id.clone(),
                        header: hunk.header.clone(),
                        old_start: hunk.old_start,
                        old_lines: hunk.old_lines,
                        new_start: hunk.new_start,
                        new_lines: hunk.new_lines,
                        lines: hunk.lines.iter().map(|line| line.view.clone()).collect(),
                        changes: hunk
                            .change_indexes
                            .iter()
                            .map(|index| {
                                let change = &file.changes[*index];
                                ChangeView {
                                    id: change.id.clone(),
                                    header: change.header.clone(),
                                    old_start: change.old_start,
                                    old_lines: change.old_lines,
                                    new_start: change.new_start,
                                    new_lines: change.new_lines,
                                    lines: change
                                        .lines
                                        .iter()
                                        .map(|line| line.view.clone())
                                        .collect(),
                                }
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
        unsupported,
    };

    Ok(ScanState { snapshot, files })
}

fn scan_path(
    repo_root: &std::path::Path,
    mode: Mode,
    path: &str,
    untracked: bool,
) -> AppResult<FileState> {
    if untracked {
        let _ = git::read_file_text(repo_root, path)?;
    }

    let display = parse_patch(&git::diff_for_path(repo_root, mode, path, 3, untracked)?)?;
    let minimal = parse_patch(&git::diff_for_path(repo_root, mode, path, 0, untracked)?)?;

    let mut hunks = display
        .hunks
        .iter()
        .map(|hunk| HunkState {
            id: String::new(),
            header: hunk.header.clone(),
            old_start: hunk.old_start,
            old_lines: hunk.old_lines,
            new_start: hunk.new_start,
            new_lines: hunk.new_lines,
            lines: hunk.lines.clone(),
            change_indexes: Vec::new(),
        })
        .collect::<Vec<_>>();

    let patch_header_lines = filter_patch_header(&minimal);
    let mut changes = Vec::new();
    for minimal_hunk in minimal.hunks {
        let hunk_index = find_parent_hunk(&hunks, &minimal_hunk).ok_or_else(|| {
            AppError::new(
                "mapping_failed",
                format!(
                    "could not map minimal hunk for '{}' back to display hunk",
                    path
                ),
            )
        })?;
        let change_index = changes.len();
        hunks[hunk_index].change_indexes.push(change_index);
        changes.push(ChangeState {
            id: String::new(),
            header: minimal_hunk.header,
            old_start: minimal_hunk.old_start,
            old_lines: minimal_hunk.old_lines,
            new_start: minimal_hunk.new_start,
            new_lines: minimal_hunk.new_lines,
            lines: minimal_hunk.lines,
        });
    }

    Ok(FileState {
        path: path.to_string(),
        status: display.status,
        patch_header_lines,
        hunks,
        changes,
    })
}

fn filter_patch_header(patch: &ParsedPatch) -> Vec<String> {
    patch
        .header_lines
        .iter()
        .filter(|line| !line.starts_with("index "))
        .cloned()
        .collect()
}

fn find_parent_hunk(hunks: &[HunkState], change: &ParsedHunk) -> Option<usize> {
    if hunks.len() == 1 {
        return Some(0);
    }

    hunks
        .iter()
        .enumerate()
        .find(|(_, hunk)| {
            coordinates_overlap(
                hunk.old_start,
                hunk.old_lines,
                change.old_start,
                change.old_lines,
            ) || coordinates_overlap(
                hunk.new_start,
                hunk.new_lines,
                change.new_start,
                change.new_lines,
            )
        })
        .map(|(index, _)| index)
}

fn coordinates_overlap(base_start: u32, base_len: u32, change_start: u32, change_len: u32) -> bool {
    let base_end = if base_len == 0 {
        base_start
    } else {
        base_start + base_len - 1
    };
    let change_end = if change_len == 0 {
        change_start
    } else {
        change_start + change_len - 1
    };

    change_start <= base_end.saturating_add(1) && change_end.saturating_add(1) >= base_start
}

fn snapshot_id(mode: Mode, files: &[FileState], unsupported: &[UnsupportedPath]) -> String {
    let mut material = format!("mode:{}\n", mode.as_str());
    for file in files {
        material.push_str(&format!("file:{}:{:?}\n", file.path, file.status));
        for hunk in &file.hunks {
            material.push_str(&format!(
                "display:{}\n{}\n",
                hunk.header,
                render_raw_lines(&hunk.lines)
            ));
        }
        for change in &file.changes {
            material.push_str(&format!(
                "change:{}\n{}\n",
                change.header,
                render_raw_lines(&change.lines)
            ));
        }
    }
    for item in unsupported {
        material.push_str(&format!("unsupported:{}:{}\n", item.path, item.reason));
    }
    short_id("s", &material)
}

fn render_raw_lines(lines: &[crate::model::PatchLine]) -> String {
    lines
        .iter()
        .map(|line| line.raw.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn short_id(prefix: &str, material: &str) -> String {
    let digest = blake3::hash(material.as_bytes()).to_hex().to_string();
    format!("{}_{}", prefix, &digest[..12])
}
