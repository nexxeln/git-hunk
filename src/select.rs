use std::collections::BTreeSet;

use crate::error::{AppError, AppResult};
use crate::model::ScanState;

#[derive(Debug, Clone)]
pub struct SelectionInput {
    pub snapshot_id: Option<String>,
    pub hunk_ids: Vec<String>,
    pub change_ids: Vec<String>,
}

impl SelectionInput {
    pub fn has_selectors(&self) -> bool {
        !self.hunk_ids.is_empty() || !self.change_ids.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedSelection {
    pub selected_hunks: Vec<String>,
    pub selected_changes: Vec<String>,
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

    for hunk_id in &input.hunk_ids {
        let mut found = false;
        for file in &state.files {
            for hunk in &file.hunks {
                if hunk.id == *hunk_id {
                    found = true;
                    selected_hunks.push(hunk_id.clone());
                    for change_index in &hunk.change_indexes {
                        selected_changes.insert((file.path.clone(), *change_index));
                    }
                }
            }
        }
        if !found {
            return Err(AppError::new(
                "unknown_hunk",
                format!("unknown hunk id '{}'", hunk_id),
            ));
        }
    }

    let mut selected_change_ids = Vec::new();
    for change_id in &input.change_ids {
        let mut found = false;
        for file in &state.files {
            for (change_index, change) in file.changes.iter().enumerate() {
                if change.id == *change_id {
                    found = true;
                    selected_change_ids.push(change_id.clone());
                    selected_changes.insert((file.path.clone(), change_index));
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
                        .contains(&(file.path.clone(), change_index))
                        .then_some(change_index)
                })
                .collect::<Vec<_>>();
            (!indexes.is_empty()).then_some((file_index, indexes))
        })
        .collect::<Vec<_>>();

    Ok(ResolvedSelection {
        selected_hunks,
        selected_changes: selected_change_ids,
        per_file_change_indexes,
    })
}
