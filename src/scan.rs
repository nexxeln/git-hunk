use std::collections::BTreeMap;

use crate::cli::Mode;
use crate::diff::{ParsedHunk, ParsedPatch, parse_patch};
use crate::error::{AppError, AppResult};
use crate::git;
use crate::model::{
    CHANGE_KEY_SCHEME, ChangeState, ChangeView, FileState, FileView, HunkState, HunkView, LineKind,
    PatchLine, ScanState, SnapshotView, UnsupportedPath, change_selector_bundle,
    hunk_selector_bundle,
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

    let selector_snapshot_id = snapshot_id.clone();
    let snapshot = SnapshotView {
        snapshot_id,
        change_key_scheme: CHANGE_KEY_SCHEME,
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
                        selectors: hunk_selector_bundle(&selector_snapshot_id, &hunk.id),
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
                                let lines = change
                                    .lines
                                    .iter()
                                    .map(|line| line.view.clone())
                                    .collect::<Vec<_>>();
                                ChangeView {
                                    id: change.id.clone(),
                                    change_key: change.change_key.clone(),
                                    selectors: change_selector_bundle(
                                        &selector_snapshot_id,
                                        &hunk.id,
                                        &change.id,
                                        &change.change_key,
                                        change.old_start,
                                        change.old_lines,
                                        change.new_start,
                                        change.new_lines,
                                    ),
                                    header: change.header.clone(),
                                    old_start: change.old_start,
                                    old_lines: change.old_lines,
                                    new_start: change.new_start,
                                    new_lines: change.new_lines,
                                    metadata: crate::model::ChangeMetadata::from_lines(&lines),
                                    lines,
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
            change_key: String::new(),
            header: minimal_hunk.header,
            old_start: minimal_hunk.old_start,
            old_lines: minimal_hunk.old_lines,
            new_start: minimal_hunk.new_start,
            new_lines: minimal_hunk.new_lines,
            lines: minimal_hunk.lines,
        });
    }

    assign_change_keys(path, display.status, &hunks, &mut changes);

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

fn assign_change_keys(
    path: &str,
    status: crate::model::FileStatus,
    hunks: &[HunkState],
    changes: &mut [ChangeState],
) {
    let mut counts = BTreeMap::<String, usize>::new();

    for hunk in hunks {
        for change_index in &hunk.change_indexes {
            let base = stable_change_key(path, status, hunk, &changes[*change_index]);
            let entry = counts.entry(base.clone()).or_insert(0);
            *entry += 1;
            changes[*change_index].change_key = if *entry == 1 {
                base
            } else {
                format!("{}-{}", base, *entry)
            };
        }
    }
}

fn stable_change_key(
    path: &str,
    status: crate::model::FileStatus,
    hunk: &HunkState,
    change: &ChangeState,
) -> String {
    let (before, after) = surrounding_context(hunk, change);
    let material = format!(
        "path:{}\nstatus:{}\nbefore:{}\nchange:{}\nafter:{}\ncounts:{}:{}",
        path,
        status.as_str(),
        before.join("\n"),
        render_raw_lines(&change.lines),
        after.join("\n"),
        change.old_lines,
        change.new_lines
    );
    short_id("ck", &material)
}

fn surrounding_context(hunk: &HunkState, change: &ChangeState) -> (Vec<String>, Vec<String>) {
    let Some((start, end)) = locate_change_lines(&hunk.lines, change) else {
        return (Vec::new(), Vec::new());
    };

    let before = hunk.lines[..start]
        .iter()
        .rev()
        .filter(|line| line.view.kind == LineKind::Context)
        .take(2)
        .map(|line| line.view.text.clone())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let after = hunk.lines[end + 1..]
        .iter()
        .filter(|line| line.view.kind == LineKind::Context)
        .take(2)
        .map(|line| line.view.text.clone())
        .collect();
    (before, after)
}

fn locate_change_lines(hunk_lines: &[PatchLine], change: &ChangeState) -> Option<(usize, usize)> {
    let change_lines = &change.lines;
    if change_lines.is_empty() || change_lines.len() > hunk_lines.len() {
        return None;
    }

    let expected_old = change.lines.iter().find_map(|line| line.view.old_lineno);
    let expected_new = change.lines.iter().find_map(|line| line.view.new_lineno);

    hunk_lines
        .windows(change_lines.len())
        .position(|window| {
            let raws_match = window
                .iter()
                .zip(change_lines.iter())
                .all(|(left, right)| left.raw == right.raw);
            let old_matches = expected_old
                .map(|expected| {
                    window.iter().find_map(|line| line.view.old_lineno) == Some(expected)
                })
                .unwrap_or(true);
            let new_matches = expected_new
                .map(|expected| {
                    window.iter().find_map(|line| line.view.new_lineno) == Some(expected)
                })
                .unwrap_or(true);

            raws_match && old_matches && new_matches
        })
        .map(|start| (start, start + change_lines.len() - 1))
}
