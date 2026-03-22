use std::collections::BTreeSet;

use serde::Serialize;

use crate::cli::Mode;
use crate::model::{
    ChangeSelectorBundle, LineSide, ScanState, SelectorRef, SnapshotOutput, change_selector_bundle,
};
use crate::select::{HunkSelector, LineRangeSelector, SelectionInput};

#[derive(Debug, Clone, Serialize)]
pub struct SelectionValidation {
    pub mode: Mode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_snapshot_id: Option<String>,
    pub snapshot_id: String,
    pub snapshot_matches: bool,
    pub stale: bool,
    pub directly_usable: bool,
    pub can_apply: bool,
    pub resolved_selectors: Vec<SelectorRef>,
    pub unresolved_selectors: Vec<SelectorRef>,
    pub matched_changes: Vec<ChangeSelectorBundle>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidateResponse {
    #[serde(flatten)]
    pub validation: SelectionValidation,
    pub snapshot: SnapshotOutput,
}

impl ValidateResponse {
    pub fn to_text(&self) -> String {
        format!(
            "validated {} selector(s): {} ready now, {} recoverable with fresh snapshot, {} unresolved\ncurrent snapshot: {}",
            self.validation.resolved_selectors.len() + self.validation.unresolved_selectors.len(),
            usize::from(self.validation.directly_usable),
            usize::from(self.validation.can_apply && !self.validation.directly_usable),
            self.validation.unresolved_selectors.len(),
            self.validation.snapshot_id,
        )
    }
}

pub fn validate_selection(
    state: &ScanState,
    selection: &SelectionInput,
    compact: bool,
) -> ValidateResponse {
    ValidateResponse {
        validation: summarize_selection(state, selection),
        snapshot: SnapshotOutput::from_snapshot(state.snapshot.clone(), compact),
    }
}

pub fn summarize_selection(state: &ScanState, selection: &SelectionInput) -> SelectionValidation {
    let requested_snapshot_id = selection.snapshot_id.clone();
    let snapshot_matches = requested_snapshot_id
        .as_ref()
        .map(|snapshot| snapshot == &state.snapshot.snapshot_id)
        .unwrap_or(true);

    let mut resolved_selectors = Vec::new();
    let mut unresolved_selectors = Vec::new();
    let mut matched_changes = Vec::new();
    let mut seen_change_ids = BTreeSet::new();

    for selector in &selection.hunks {
        match selector {
            HunkSelector::Whole { id } => {
                let selector_ref = SelectorRef::Hunk { id: id.clone() };
                if let Some((file_index, hunk_index)) = find_hunk(state, id) {
                    resolved_selectors.push(selector_ref);
                    let file = &state.files[file_index];
                    let hunk = &file.hunks[hunk_index];
                    for change_index in &hunk.change_indexes {
                        push_change_bundle(
                            state,
                            file_index,
                            hunk_index,
                            *change_index,
                            &mut seen_change_ids,
                            &mut matched_changes,
                        );
                    }
                } else {
                    unresolved_selectors.push(selector_ref);
                }
            }
            HunkSelector::LineRange(selector) => {
                let selector_ref = selector_ref_for_line_range(selector);
                if let Some((file_index, hunk_index)) = find_hunk(state, &selector.hunk_id) {
                    let file = &state.files[file_index];
                    let hunk = &file.hunks[hunk_index];
                    let mut matched = false;
                    let mut partial = false;

                    for change_index in &hunk.change_indexes {
                        let change = &file.changes[*change_index];
                        match range_match(change, selector.side, selector.start, selector.end) {
                            RangeMatch::None => {}
                            RangeMatch::Full => {
                                matched = true;
                                push_change_bundle(
                                    state,
                                    file_index,
                                    hunk_index,
                                    *change_index,
                                    &mut seen_change_ids,
                                    &mut matched_changes,
                                );
                            }
                            RangeMatch::Partial => partial = true,
                        }
                    }

                    if matched && !partial {
                        resolved_selectors.push(selector_ref);
                    } else {
                        unresolved_selectors.push(selector_ref);
                    }
                } else {
                    unresolved_selectors.push(selector_ref);
                }
            }
        }
    }

    for change_id in &selection.change_ids {
        let selector_ref = SelectorRef::Change {
            id: change_id.clone(),
        };
        if let Some((file_index, hunk_index, change_index)) = find_change_by_id(state, change_id) {
            resolved_selectors.push(selector_ref);
            push_change_bundle(
                state,
                file_index,
                hunk_index,
                change_index,
                &mut seen_change_ids,
                &mut matched_changes,
            );
        } else {
            unresolved_selectors.push(selector_ref);
        }
    }

    for change_key in &selection.change_keys {
        let selector_ref = SelectorRef::ChangeKey {
            key: change_key.clone(),
            scheme: crate::model::CHANGE_KEY_SCHEME,
        };
        if let Some((file_index, hunk_index, change_index)) = find_change_by_key(state, change_key)
        {
            resolved_selectors.push(selector_ref);
            push_change_bundle(
                state,
                file_index,
                hunk_index,
                change_index,
                &mut seen_change_ids,
                &mut matched_changes,
            );
        } else {
            unresolved_selectors.push(selector_ref);
        }
    }

    let stale = requested_snapshot_id.is_some() && !snapshot_matches;
    let directly_usable = snapshot_matches && unresolved_selectors.is_empty();
    let can_apply = unresolved_selectors.is_empty();

    SelectionValidation {
        mode: state.snapshot.mode,
        requested_snapshot_id,
        snapshot_id: state.snapshot.snapshot_id.clone(),
        snapshot_matches,
        stale,
        directly_usable,
        can_apply,
        resolved_selectors,
        unresolved_selectors,
        matched_changes,
    }
}

fn push_change_bundle(
    state: &ScanState,
    file_index: usize,
    hunk_index: usize,
    change_index: usize,
    seen_change_ids: &mut BTreeSet<String>,
    matched_changes: &mut Vec<ChangeSelectorBundle>,
) {
    let file = &state.files[file_index];
    let hunk = &file.hunks[hunk_index];
    let change = &file.changes[change_index];
    if seen_change_ids.insert(change.id.clone()) {
        matched_changes.push(change_selector_bundle(
            &state.snapshot.snapshot_id,
            &hunk.id,
            &change.id,
            &change.change_key,
            change.old_start,
            change.old_lines,
            change.new_start,
            change.new_lines,
        ));
    }
}

fn find_hunk(state: &ScanState, id: &str) -> Option<(usize, usize)> {
    state
        .files
        .iter()
        .enumerate()
        .find_map(|(file_index, file)| {
            file.hunks
                .iter()
                .enumerate()
                .find(|(_, hunk)| hunk.id == id)
                .map(|(hunk_index, _)| (file_index, hunk_index))
        })
}

fn find_change_by_id(state: &ScanState, id: &str) -> Option<(usize, usize, usize)> {
    for (file_index, file) in state.files.iter().enumerate() {
        for (hunk_index, hunk) in file.hunks.iter().enumerate() {
            for change_index in &hunk.change_indexes {
                if file.changes[*change_index].id == id {
                    return Some((file_index, hunk_index, *change_index));
                }
            }
        }
    }
    None
}

fn find_change_by_key(state: &ScanState, key: &str) -> Option<(usize, usize, usize)> {
    for (file_index, file) in state.files.iter().enumerate() {
        for (hunk_index, hunk) in file.hunks.iter().enumerate() {
            for change_index in &hunk.change_indexes {
                if file.changes[*change_index].change_key == key {
                    return Some((file_index, hunk_index, *change_index));
                }
            }
        }
    }
    None
}

fn selector_ref_for_line_range(selector: &LineRangeSelector) -> SelectorRef {
    SelectorRef::LineRange {
        hunk_id: selector.hunk_id.clone(),
        side: selector.side,
        start: selector.start,
        end: selector.end,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RangeMatch {
    None,
    Full,
    Partial,
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
