use std::collections::BTreeSet;

use crate::error::{AppError, AppResult};
use crate::model::{LineSide, ScanState};

#[derive(Debug, Clone)]
pub struct SelectionInput {
    pub snapshot_id: Option<String>,
    pub hunks: Vec<HunkSelector>,
    pub change_ids: Vec<String>,
    pub change_keys: Vec<String>,
}

impl SelectionInput {
    pub fn has_selectors(&self) -> bool {
        !self.hunks.is_empty() || !self.change_ids.is_empty() || !self.change_keys.is_empty()
    }
}

#[derive(Debug, Clone)]
pub enum HunkSelector {
    Whole { id: String },
    LineRange(LineRangeSelector),
}

#[derive(Debug, Clone)]
pub struct LineRangeSelector {
    pub hunk_id: String,
    pub side: LineSide,
    pub start: u32,
    pub end: u32,
}

impl LineRangeSelector {
    pub fn display(&self) -> String {
        format!(
            "{}:{}:{}-{}",
            self.hunk_id,
            self.side.as_str(),
            self.start,
            self.end
        )
    }
}

impl HunkSelector {
    pub fn parse(raw: &str) -> AppResult<Self> {
        let parts = raw.split(':').collect::<Vec<_>>();
        match parts.as_slice() {
            [id] => Ok(Self::Whole {
                id: (*id).to_string(),
            }),
            [hunk_id, side, range] => {
                let side = parse_side(side)?;
                let (start, end) = parse_line_range(range)?;
                Ok(Self::LineRange(LineRangeSelector {
                    hunk_id: (*hunk_id).to_string(),
                    side,
                    start,
                    end,
                }))
            }
            _ => Err(AppError::new(
                "invalid_hunk_selector",
                format!(
                    "invalid hunk selector '{}'; use <id> or <id>:<old|new>:<start-end>",
                    raw
                ),
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedSelection {
    pub selected_hunks: Vec<String>,
    pub selected_changes: Vec<String>,
    pub selected_change_keys: Vec<String>,
    pub selected_line_ranges: Vec<String>,
    pub per_file_change_indexes: Vec<(usize, Vec<usize>)>,
}

pub fn resolve_selection(
    state: &ScanState,
    input: &SelectionInput,
) -> AppResult<ResolvedSelection> {
    if !input.has_selectors() {
        return Err(AppError::new(
            "missing_selection",
            "provide at least one --hunk or --change selector".to_string(),
        ));
    }

    let mut selected_hunks = Vec::new();
    let mut selected_changes = BTreeSet::new();
    let mut selected_change_keys = Vec::new();
    let mut selected_line_ranges = Vec::new();

    for selector in &input.hunks {
        match selector {
            HunkSelector::Whole { id } => {
                let mut found = false;
                for (file_index, file) in state.files.iter().enumerate() {
                    for hunk in &file.hunks {
                        if hunk.id == *id {
                            found = true;
                            selected_hunks.push(id.clone());
                            for change_index in &hunk.change_indexes {
                                selected_changes.insert((file_index, *change_index));
                            }
                        }
                    }
                }
                if !found {
                    return Err(AppError::new(
                        "unknown_hunk",
                        format!("unknown hunk id '{}'", id),
                    ));
                }
            }
            HunkSelector::LineRange(selector) => {
                let (file_index, hunk) = find_hunk(state, &selector.hunk_id).ok_or_else(|| {
                    AppError::new(
                        "unknown_hunk",
                        format!("unknown hunk id '{}'", selector.hunk_id),
                    )
                })?;

                let file = &state.files[file_index];
                let mut matched = false;
                let mut partial_matches = Vec::new();

                for change_index in &hunk.change_indexes {
                    let change = &file.changes[*change_index];
                    match range_match(change, selector.side, selector.start, selector.end) {
                        RangeMatch::None => {}
                        RangeMatch::Full => {
                            matched = true;
                            selected_changes.insert((file_index, *change_index));
                        }
                        RangeMatch::Partial => partial_matches.push(change.id.clone()),
                    }
                }

                if !partial_matches.is_empty() {
                    return Err(AppError::new(
                        "ambiguous_line_range",
                        format!(
                            "{} only partially covers change(s) {}; select full change blocks or use their ids directly",
                            selector.display(),
                            partial_matches.join(", ")
                        ),
                    ));
                }

                if !matched {
                    return Err(AppError::new(
                        "empty_line_range",
                        format!(
                            "{} does not fully cover any selectable change in hunk '{}'",
                            selector.display(),
                            selector.hunk_id
                        ),
                    ));
                }

                selected_line_ranges.push(selector.display());
            }
        }
    }

    let mut selected_change_ids = Vec::new();
    for change_id in &input.change_ids {
        let mut found = false;
        for (file_index, file) in state.files.iter().enumerate() {
            for (change_index, change) in file.changes.iter().enumerate() {
                if change.id == *change_id {
                    found = true;
                    selected_change_ids.push(change_id.clone());
                    selected_changes.insert((file_index, change_index));
                }
            }
        }
        if !found {
            return Err(AppError::new(
                "unknown_change",
                format!("unknown change id '{}'", change_id),
            ));
        }
    }

    for change_key in &input.change_keys {
        let mut found = false;
        for (file_index, file) in state.files.iter().enumerate() {
            for (change_index, change) in file.changes.iter().enumerate() {
                if change.change_key == *change_key {
                    found = true;
                    selected_change_keys.push(change_key.clone());
                    selected_changes.insert((file_index, change_index));
                }
            }
        }
        if !found {
            return Err(AppError::new(
                "unknown_change_key",
                format!("unknown change key '{}'", change_key),
            ));
        }
    }

    let per_file_change_indexes = state
        .files
        .iter()
        .enumerate()
        .filter_map(|(file_index, file)| {
            let indexes = file
                .changes
                .iter()
                .enumerate()
                .filter_map(|(change_index, _)| {
                    selected_changes
                        .contains(&(file_index, change_index))
                        .then_some(change_index)
                })
                .collect::<Vec<_>>();
            (!indexes.is_empty()).then_some((file_index, indexes))
        })
        .collect::<Vec<_>>();

    Ok(ResolvedSelection {
        selected_hunks,
        selected_changes: selected_change_ids,
        selected_change_keys,
        selected_line_ranges,
        per_file_change_indexes,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeMatch {
    None,
    Full,
    Partial,
}

fn parse_side(raw: &str) -> AppResult<LineSide> {
    match raw {
        "old" => Ok(LineSide::Old),
        "new" => Ok(LineSide::New),
        _ => Err(AppError::new(
            "invalid_hunk_selector",
            format!("unknown line side '{}'; expected 'old' or 'new'", raw),
        )),
    }
}

fn parse_line_range(raw: &str) -> AppResult<(u32, u32)> {
    let (start, end) = raw.split_once('-').ok_or_else(|| {
        AppError::new(
            "invalid_hunk_selector",
            format!("invalid line range '{}'; expected <start-end>", raw),
        )
    })?;
    let start = start.parse::<u32>().map_err(|_| {
        AppError::new(
            "invalid_hunk_selector",
            format!("invalid line range start '{}'", start),
        )
    })?;
    let end = end.parse::<u32>().map_err(|_| {
        AppError::new(
            "invalid_hunk_selector",
            format!("invalid line range end '{}'", end),
        )
    })?;
    if start > end {
        return Err(AppError::new(
            "invalid_hunk_selector",
            format!("invalid line range '{}'; start must be <= end", raw),
        ));
    }
    Ok((start, end))
}

fn find_hunk<'a>(state: &'a ScanState, id: &str) -> Option<(usize, &'a crate::model::HunkState)> {
    state
        .files
        .iter()
        .enumerate()
        .find_map(|(file_index, file)| {
            file.hunks
                .iter()
                .find(|hunk| hunk.id == id)
                .map(|hunk| (file_index, hunk))
        })
}

fn range_match(
    change: &crate::model::ChangeState,
    side: LineSide,
    start: u32,
    end: u32,
) -> RangeMatch {
    let (change_start, change_lines) = match side {
        LineSide::Old => (change.old_start, change.old_lines),
        LineSide::New => (change.new_start, change.new_lines),
    };

    if change_lines == 0 {
        return RangeMatch::None;
    }

    let change_end = change_start + change_lines - 1;
    if end < change_start || start > change_end {
        return RangeMatch::None;
    }

    if start <= change_start && end >= change_end {
        RangeMatch::Full
    } else {
        RangeMatch::Partial
    }
}
