use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::json;

use crate::cli::{Mode, ResolveSide};
use crate::error::{AppError, AppResult};
use crate::model::{
    CHANGE_KEY_SCHEME, ChangeMetadata, ChangeSelectorBundle, LineSide, ScanState,
    change_selector_bundle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveStatus {
    Exact,
    Adjusted,
    Nearest,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveCandidate {
    pub change_id: String,
    pub change_key: String,
    pub hunk_id: String,
    pub selectors: ChangeSelectorBundle,
    pub side: LineSide,
    pub start: u32,
    pub end: u32,
    pub distance: u32,
    pub metadata: ChangeMetadata,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveResponse {
    pub snapshot_id: String,
    pub change_key_scheme: &'static str,
    pub mode: Mode,
    pub path: String,
    pub requested_side: ResolveSide,
    pub requested_start: u32,
    pub requested_end: u32,
    pub matched_side: LineSide,
    pub status: ResolveStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hunk_id: Option<String>,
    pub recommended_change_ids: Vec<String>,
    pub recommended_change_keys: Vec<String>,
    pub recommended_hunk_selectors: Vec<String>,
    pub recommended_selectors: Vec<ChangeSelectorBundle>,
    pub candidates: Vec<ResolveCandidate>,
}

impl ResolveResponse {
    pub fn to_text(&self) -> String {
        format!(
            "resolved {}:{}-{} to {} change(s) on {} [{}]",
            self.path,
            self.requested_start,
            self.requested_end,
            self.recommended_change_ids.len(),
            self.matched_side.as_str(),
            match self.status {
                ResolveStatus::Exact => "exact",
                ResolveStatus::Adjusted => "adjusted",
                ResolveStatus::Nearest => "nearest",
            }
        )
    }
}

pub fn resolve_region(
    state: &ScanState,
    path: &str,
    start: u32,
    end: u32,
    requested_side: ResolveSide,
) -> AppResult<ResolveResponse> {
    if start > end {
        return Err(AppError::new(
            "invalid_resolve_range",
            format!(
                "resolve range {}-{} is invalid; start must be <= end",
                start, end
            ),
        ));
    }

    let file_index = state
        .files
        .iter()
        .position(|file| file.path == path)
        .ok_or_else(|| {
            AppError::new(
                "unknown_path",
                format!("no changed path found for '{}' in snapshot", path),
            )
            .with_details(json!({
                "path": path,
                "changed_paths": state.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(),
            }))
        })?;

    let file = &state.files[file_index];
    if file.changes.is_empty() {
        return Err(AppError::new(
            "no_changes_in_path",
            format!("'{}' has no selectable changes", path),
        )
        .with_details(json!({ "path": path })));
    }

    let side_results = match requested_side {
        ResolveSide::Auto => vec![
            resolve_for_side(state, file_index, LineSide::New, start, end),
            resolve_for_side(state, file_index, LineSide::Old, start, end),
        ],
        ResolveSide::Old => vec![resolve_for_side(
            state,
            file_index,
            LineSide::Old,
            start,
            end,
        )],
        ResolveSide::New => vec![resolve_for_side(
            state,
            file_index,
            LineSide::New,
            start,
            end,
        )],
    }
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    if side_results.is_empty() {
        return Err(AppError::new(
            "no_resolve_candidates",
            format!(
                "'{}' has no line-addressable changes for the requested side",
                path
            ),
        )
        .with_details(json!({ "path": path, "requested_side": requested_side.as_str() })));
    }

    let preferred = preferred_side(state.snapshot.mode);
    let best = side_results
        .into_iter()
        .min_by(|left, right| compare_resolution(left, right, preferred))
        .expect("side results should not be empty");

    Ok(ResolveResponse {
        snapshot_id: state.snapshot.snapshot_id.clone(),
        change_key_scheme: CHANGE_KEY_SCHEME,
        mode: state.snapshot.mode,
        path: file.path.clone(),
        requested_side,
        requested_start: start,
        requested_end: end,
        matched_side: best.side,
        status: best.status,
        hunk_id: (best.hunk_ids.len() == 1).then(|| best.hunk_ids[0].clone()),
        recommended_change_ids: best.change_ids,
        recommended_change_keys: best.change_keys,
        recommended_hunk_selectors: best.hunk_selectors,
        recommended_selectors: best.selector_bundles,
        candidates: best.candidates,
    })
}

#[derive(Debug, Clone)]
struct SideResolution {
    side: LineSide,
    status: ResolveStatus,
    distance: u32,
    adjustment: u32,
    change_ids: Vec<String>,
    change_keys: Vec<String>,
    hunk_ids: Vec<String>,
    hunk_selectors: Vec<String>,
    selector_bundles: Vec<ChangeSelectorBundle>,
    candidates: Vec<ResolveCandidate>,
}

fn resolve_for_side(
    state: &ScanState,
    file_index: usize,
    side: LineSide,
    start: u32,
    end: u32,
) -> Option<SideResolution> {
    let file = &state.files[file_index];
    let mut overlapping = Vec::new();
    let mut nearest = Vec::new();
    let mut min_distance = u32::MAX;

    for (change_index, change) in file.changes.iter().enumerate() {
        let (range_start, range_len) = match side {
            LineSide::Old => (change.old_start, change.old_lines),
            LineSide::New => (change.new_start, change.new_lines),
        };
        if range_len == 0 {
            continue;
        }
        let range_end = range_start + range_len - 1;
        let distance = range_distance(start, end, range_start, range_end);

        if distance == 0 {
            overlapping.push((change_index, range_start, range_end));
        }

        if distance < min_distance {
            min_distance = distance;
            nearest.clear();
            nearest.push((change_index, range_start, range_end, distance));
        } else if distance == min_distance {
            nearest.push((change_index, range_start, range_end, distance));
        }
    }

    if !overlapping.is_empty() {
        return Some(build_resolution(
            state,
            file_index,
            side,
            ResolveStatus::from_exactness(start, end, &overlapping),
            0,
            adjustment_distance(start, end, &overlapping),
            overlapping
                .into_iter()
                .map(|(change_index, range_start, range_end)| {
                    (change_index, range_start, range_end, 0)
                })
                .collect(),
        ));
    }

    if nearest.is_empty() {
        None
    } else {
        Some(build_resolution(
            state,
            file_index,
            side,
            ResolveStatus::Nearest,
            min_distance,
            adjustment_distance_from_entries(start, end, &nearest),
            nearest,
        ))
    }
}

fn build_resolution(
    state: &ScanState,
    file_index: usize,
    side: LineSide,
    status: ResolveStatus,
    distance: u32,
    adjustment: u32,
    entries: Vec<(usize, u32, u32, u32)>,
) -> SideResolution {
    let file = &state.files[file_index];
    let mut hunk_ranges = BTreeMap::<usize, (u32, u32)>::new();
    let mut hunk_ids = Vec::new();
    let mut change_ids = Vec::new();
    let mut change_keys = Vec::new();
    let mut selector_bundles = Vec::new();
    let mut candidates = Vec::new();

    for (change_index, range_start, range_end, change_distance) in entries {
        let change = &file.changes[change_index];
        let hunk_index = file
            .hunks
            .iter()
            .position(|hunk| hunk.change_indexes.contains(&change_index))
            .expect("change should belong to a hunk");
        let hunk = &file.hunks[hunk_index];

        if !hunk_ids.iter().any(|id| id == &hunk.id) {
            hunk_ids.push(hunk.id.clone());
        }
        change_ids.push(change.id.clone());
        change_keys.push(change.change_key.clone());
        let selectors = change_selector_bundle(
            &state.snapshot.snapshot_id,
            &hunk.id,
            &change.id,
            &change.change_key,
            change.old_start,
            change.old_lines,
            change.new_start,
            change.new_lines,
        );
        selector_bundles.push(selectors.clone());
        candidates.push(ResolveCandidate {
            change_id: change.id.clone(),
            change_key: change.change_key.clone(),
            hunk_id: hunk.id.clone(),
            selectors,
            side,
            start: range_start,
            end: range_end,
            distance: change_distance,
            metadata: ChangeMetadata::from_lines(
                &change
                    .lines
                    .iter()
                    .map(|line| line.view.clone())
                    .collect::<Vec<_>>(),
            ),
        });

        hunk_ranges
            .entry(hunk_index)
            .and_modify(|range| {
                range.0 = range.0.min(range_start);
                range.1 = range.1.max(range_end);
            })
            .or_insert((range_start, range_end));
    }

    let hunk_selectors = hunk_ranges
        .into_iter()
        .map(|(hunk_index, (range_start, range_end))| {
            format!(
                "{}:{}:{}-{}",
                file.hunks[hunk_index].id,
                side.as_str(),
                range_start,
                range_end
            )
        })
        .collect();

    SideResolution {
        side,
        status,
        distance,
        adjustment,
        change_ids,
        change_keys,
        hunk_ids,
        hunk_selectors,
        selector_bundles,
        candidates,
    }
}

fn compare_resolution(
    left: &SideResolution,
    right: &SideResolution,
    preferred: LineSide,
) -> Ordering {
    status_rank(left.status)
        .cmp(&status_rank(right.status))
        .then(left.distance.cmp(&right.distance))
        .then(left.adjustment.cmp(&right.adjustment))
        .then(left.change_ids.len().cmp(&right.change_ids.len()))
        .then(side_preference(left.side, preferred).cmp(&side_preference(right.side, preferred)))
}

fn status_rank(status: ResolveStatus) -> u8 {
    match status {
        ResolveStatus::Exact => 0,
        ResolveStatus::Adjusted => 1,
        ResolveStatus::Nearest => 2,
    }
}

fn side_preference(side: LineSide, preferred: LineSide) -> u8 {
    if side == preferred { 0 } else { 1 }
}

fn preferred_side(mode: Mode) -> LineSide {
    match mode {
        Mode::Stage => LineSide::New,
        Mode::Unstage => LineSide::Old,
    }
}

fn range_distance(start: u32, end: u32, candidate_start: u32, candidate_end: u32) -> u32 {
    if end < candidate_start {
        candidate_start - end
    } else if start > candidate_end {
        start - candidate_end
    } else {
        0
    }
}

fn adjustment_distance(start: u32, end: u32, entries: &[(usize, u32, u32)]) -> u32 {
    let union_start = entries
        .iter()
        .map(|(_, value, _)| *value)
        .min()
        .unwrap_or(start);
    let union_end = entries
        .iter()
        .map(|(_, _, value)| *value)
        .max()
        .unwrap_or(end);
    start.abs_diff(union_start) + end.abs_diff(union_end)
}

fn adjustment_distance_from_entries(
    start: u32,
    end: u32,
    entries: &[(usize, u32, u32, u32)],
) -> u32 {
    let union_start = entries
        .iter()
        .map(|(_, value, _, _)| *value)
        .min()
        .unwrap_or(start);
    let union_end = entries
        .iter()
        .map(|(_, _, value, _)| *value)
        .max()
        .unwrap_or(end);
    start.abs_diff(union_start) + end.abs_diff(union_end)
}

impl ResolveStatus {
    fn from_exactness(start: u32, end: u32, entries: &[(usize, u32, u32)]) -> Self {
        let union_start = entries
            .iter()
            .map(|(_, value, _)| *value)
            .min()
            .unwrap_or(start);
        let union_end = entries
            .iter()
            .map(|(_, _, value)| *value)
            .max()
            .unwrap_or(end);
        if union_start == start && union_end == end {
            ResolveStatus::Exact
        } else {
            ResolveStatus::Adjusted
        }
    }
}
