pub mod cli;
mod diff;
mod error;
mod git;
mod model;
mod patch;
mod resolve;
mod scan;
mod select;
mod validate;

use std::io::Read;
use std::path::PathBuf;

use cli::{Cli, Command, CommitArgs, MutateArgs, ResolveArgs, ScanArgs, ShowArgs, ValidateArgs};
use error::{AppError, AppResult};
use model::{ChangeView, HunkView, ScanState, SelectionPlan, SnapshotOutput};
use select::{HunkSelector, SelectionInput};
use serde::Serialize;

pub use error::AppError as Error;

pub fn run(cli: Cli) -> AppResult<CommandOutput> {
    let repo_root = git::repo_root(&std::env::current_dir().map_err(AppError::io)?)?;

    match cli.command {
        Command::Scan(args) => scan_command(&repo_root, args),
        Command::Show(args) => show_command(&repo_root, args),
        Command::Resolve(args) => resolve_command(&repo_root, args),
        Command::Validate(args) => validate_command(&repo_root, args),
        Command::Stage(args) => mutate_command(&repo_root, args, false),
        Command::Unstage(args) => mutate_command(&repo_root, args, true),
        Command::Commit(args) => commit_command(&repo_root, args),
    }
}

fn scan_command(repo_root: &PathBuf, args: ScanArgs) -> AppResult<CommandOutput> {
    let state = scan::scan_repo(repo_root, args.mode)?;
    Ok(CommandOutput::Scan(SnapshotOutput::from_snapshot(
        state.snapshot,
        args.compact,
    )))
}

fn show_command(repo_root: &PathBuf, args: ShowArgs) -> AppResult<CommandOutput> {
    let state = scan::scan_repo(repo_root, args.mode)?;

    if let Some((file, hunk)) = state.find_hunk(&args.id) {
        return Ok(CommandOutput::Show(ShowResponse::Hunk {
            snapshot_id: state.snapshot.snapshot_id.clone(),
            mode: state.snapshot.mode,
            path: file.path.clone(),
            status: file.status,
            hunk: hunk.clone(),
        }));
    }

    if let Some((file, change)) = state.find_change(&args.id) {
        return Ok(CommandOutput::Show(ShowResponse::Change {
            snapshot_id: state.snapshot.snapshot_id.clone(),
            mode: state.snapshot.mode,
            path: file.path.clone(),
            status: file.status,
            change: change.clone(),
        }));
    }

    if let Some((file, change)) = state.find_change_key(&args.id) {
        return Ok(CommandOutput::Show(ShowResponse::Change {
            snapshot_id: state.snapshot.snapshot_id.clone(),
            mode: state.snapshot.mode,
            path: file.path.clone(),
            status: file.status,
            change: change.clone(),
        }));
    }

    Err(AppError::new(
        "unknown_id",
        format!("no hunk or change found for id '{}'", args.id),
    ))
}

fn resolve_command(repo_root: &PathBuf, args: ResolveArgs) -> AppResult<CommandOutput> {
    let selection = SelectionInput {
        snapshot_id: Some(args.snapshot),
        hunks: Vec::new(),
        change_ids: Vec::new(),
        change_keys: Vec::new(),
    };
    let state = validate_snapshot(repo_root, args.mode, &selection)?;
    let response = resolve::resolve_region(
        &state,
        &args.path,
        args.start,
        args.end.unwrap_or(args.start),
        args.side,
    )?;
    Ok(CommandOutput::Resolve(response))
}

fn validate_command(repo_root: &PathBuf, args: ValidateArgs) -> AppResult<CommandOutput> {
    let selection = load_selection_input(
        args.snapshot,
        args.plan,
        args.hunks,
        args.changes,
        args.change_keys,
    )?;
    let state = scan::scan_repo(repo_root, args.mode)?;
    Ok(CommandOutput::Validate(validate::validate_selection(
        &state,
        &selection,
        args.compact,
    )))
}

fn mutate_command(
    repo_root: &PathBuf,
    args: MutateArgs,
    reverse: bool,
) -> AppResult<CommandOutput> {
    let mode = if reverse {
        cli::Mode::Unstage
    } else {
        cli::Mode::Stage
    };
    let selection = load_selection_input(
        args.snapshot,
        args.plan,
        args.hunks,
        args.changes,
        args.change_keys,
    )?;
    let state = validate_snapshot(repo_root, mode, &selection)?;
    let resolved = select::resolve_selection(&state, &selection)?;
    let patch = patch::build_patch(&state, &resolved)?;

    if args.dry_run {
        let preview = git::preview_index(repo_root, Some(&patch), reverse)?;
        return Ok(CommandOutput::MutationDryRun(MutationDryRunResponse {
            action: if reverse { "unstage" } else { "stage" },
            dry_run: true,
            snapshot_id: state.snapshot.snapshot_id.clone(),
            mode,
            selected_hunks: resolved.selected_hunks,
            selected_changes: resolved.selected_changes,
            selected_change_keys: resolved.selected_change_keys,
            selected_line_ranges: resolved.selected_line_ranges,
            files: preview.files,
            patch: preview.patch,
            diffstat: preview.diffstat,
        }));
    }

    git::apply_patch(repo_root, &patch, reverse)?;

    let next_state = scan::scan_repo(repo_root, mode)?;
    Ok(CommandOutput::Mutation(MutationResponse {
        action: if reverse { "unstage" } else { "stage" },
        snapshot_id: next_state.snapshot.snapshot_id.clone(),
        mode,
        selected_hunks: resolved.selected_hunks,
        selected_changes: resolved.selected_changes,
        selected_change_keys: resolved.selected_change_keys,
        selected_line_ranges: resolved.selected_line_ranges,
        snapshot: SnapshotOutput::from_snapshot(next_state.snapshot, args.compact),
    }))
}

fn commit_command(repo_root: &PathBuf, args: CommitArgs) -> AppResult<CommandOutput> {
    if args.messages.is_empty() {
        return Err(AppError::new(
            "missing_message",
            "commit requires at least one message".to_string(),
        ));
    }

    let selection = load_selection_input(
        args.snapshot,
        args.plan,
        args.hunks,
        args.changes,
        args.change_keys,
    )?;
    let prepared = prepare_commit_selection(repo_root, &selection)?;

    if args.dry_run {
        let preview = git::preview_commit(repo_root, prepared.patch.as_deref(), args.allow_empty)?;
        return Ok(CommandOutput::CommitDryRun(CommitDryRunResponse {
            dry_run: true,
            snapshot_id: prepared.snapshot_id,
            messages: args.messages,
            selected_hunks: prepared.selected_hunks,
            selected_changes: prepared.selected_changes,
            selected_change_keys: prepared.selected_change_keys,
            selected_line_ranges: prepared.selected_line_ranges,
            files: preview.files,
            patch: preview.patch,
            diffstat: preview.diffstat,
        }));
    }

    if let Some(patch) = prepared.patch.as_deref() {
        git::apply_patch(repo_root, patch, false)?;
    }

    if !args.allow_empty && !git::has_staged_changes(repo_root)? {
        return Err(AppError::new(
            "nothing_staged",
            "there are no staged changes to commit".to_string(),
        ));
    }

    let commit_sha = git::commit(repo_root, &args.messages, args.allow_empty)?;
    let next_state = scan::scan_repo(repo_root, cli::Mode::Stage)?;

    Ok(CommandOutput::Commit(CommitResponse {
        commit: commit_sha,
        snapshot_id: next_state.snapshot.snapshot_id.clone(),
        selected_hunks: prepared.selected_hunks,
        selected_changes: prepared.selected_changes,
        selected_change_keys: prepared.selected_change_keys,
        selected_line_ranges: prepared.selected_line_ranges,
        snapshot: SnapshotOutput::from_snapshot(next_state.snapshot, args.compact),
    }))
}

fn prepare_commit_selection(
    repo_root: &PathBuf,
    selection: &SelectionInput,
) -> AppResult<PreparedCommitSelection> {
    if selection.has_selectors() {
        let state = validate_snapshot(repo_root, cli::Mode::Stage, selection)?;
        let resolved = select::resolve_selection(&state, selection)?;
        let patch = patch::build_patch(&state, &resolved)?;
        return Ok(PreparedCommitSelection {
            snapshot_id: state.snapshot.snapshot_id.clone(),
            patch: Some(patch),
            selected_hunks: resolved.selected_hunks,
            selected_changes: resolved.selected_changes,
            selected_change_keys: resolved.selected_change_keys,
            selected_line_ranges: resolved.selected_line_ranges,
        });
    }

    let state = scan::scan_repo(repo_root, cli::Mode::Stage)?;
    if let Some(snapshot_id) = selection.snapshot_id.as_ref() {
        if state.snapshot.snapshot_id != *snapshot_id {
            return Err(stale_snapshot_error(
                cli::Mode::Stage,
                snapshot_id,
                &state,
                selection,
            ));
        }
    }

    Ok(PreparedCommitSelection {
        snapshot_id: state.snapshot.snapshot_id,
        patch: None,
        selected_hunks: Vec::new(),
        selected_changes: Vec::new(),
        selected_change_keys: Vec::new(),
        selected_line_ranges: Vec::new(),
    })
}

fn validate_snapshot(
    repo_root: &PathBuf,
    mode: cli::Mode,
    selection: &SelectionInput,
) -> AppResult<ScanState> {
    let snapshot_id = selection.snapshot_id.as_ref().ok_or_else(|| {
        AppError::new(
            "missing_snapshot",
            "mutating commands require --snapshot or a plan with snapshot_id".to_string(),
        )
    })?;

    let state = scan::scan_repo(repo_root, mode)?;
    if state.snapshot.snapshot_id != *snapshot_id {
        return Err(stale_snapshot_error(mode, snapshot_id, &state, selection));
    }
    Ok(state)
}

fn stale_snapshot_error(
    mode: cli::Mode,
    requested_snapshot: &str,
    state: &ScanState,
    selection: &SelectionInput,
) -> AppError {
    let validation = validate::summarize_selection(state, selection);
    AppError::new(
        "stale_snapshot",
        format!(
            "snapshot '{}' no longer matches the current {} view '{}'",
            requested_snapshot,
            mode.as_str(),
            state.snapshot.snapshot_id.as_str()
        ),
    )
    .with_details(serde_json::json!({
        "mode": mode.as_str(),
        "requested_snapshot_id": requested_snapshot,
        "current_snapshot_id": state.snapshot.snapshot_id,
        "snapshot_matches": validation.snapshot_matches,
        "directly_usable": validation.directly_usable,
        "can_apply": validation.can_apply,
        "resolved_selectors": validation.resolved_selectors,
        "unresolved_selectors": validation.unresolved_selectors,
        "matched_changes": validation.matched_changes,
    }))
}

fn load_selection_input(
    snapshot: Option<String>,
    plan_path: Option<PathBuf>,
    hunks: Vec<String>,
    changes: Vec<String>,
    change_keys: Vec<String>,
) -> AppResult<SelectionInput> {
    let mut input = SelectionInput {
        snapshot_id: snapshot,
        hunks: hunks
            .into_iter()
            .map(|raw| HunkSelector::parse(&raw))
            .collect::<AppResult<Vec<_>>>()?,
        change_ids: changes,
        change_keys,
    };

    if let Some(path) = plan_path {
        let display = path.display().to_string();
        let contents = if path == PathBuf::from("-") {
            let mut contents = String::new();
            std::io::stdin()
                .read_to_string(&mut contents)
                .map_err(|err| {
                    AppError::new(
                        "plan_read_failed",
                        format!("failed to read {}: {}", display, err),
                    )
                })?;
            contents
        } else {
            std::fs::read_to_string(&path).map_err(|err| {
                AppError::new(
                    "plan_read_failed",
                    format!("failed to read {}: {}", display, err),
                )
            })?
        };
        let plan: SelectionPlan = serde_json::from_str(&contents).map_err(|err| {
            AppError::new(
                "plan_parse_failed",
                format!("failed to parse {}: {}", display, err),
            )
        })?;

        if input.snapshot_id.is_none() {
            input.snapshot_id = Some(plan.snapshot_id);
        }
        for selector in plan.selectors {
            match selector {
                model::PlanSelector::Hunk { id } => input.hunks.push(HunkSelector::Whole { id }),
                model::PlanSelector::Change { id } => input.change_ids.push(id),
                model::PlanSelector::ChangeKey { key } => input.change_keys.push(key),
                model::PlanSelector::LineRange {
                    hunk_id,
                    side,
                    start,
                    end,
                } => input
                    .hunks
                    .push(select::HunkSelector::LineRange(select::LineRangeSelector {
                        hunk_id,
                        side,
                        start,
                        end,
                    })),
            }
        }
    }

    Ok(input)
}

struct PreparedCommitSelection {
    snapshot_id: String,
    patch: Option<String>,
    selected_hunks: Vec<String>,
    selected_changes: Vec<String>,
    selected_change_keys: Vec<String>,
    selected_line_ranges: Vec<String>,
}

#[derive(Debug)]
pub enum CommandOutput {
    Scan(SnapshotOutput),
    Show(ShowResponse),
    Resolve(resolve::ResolveResponse),
    Validate(validate::ValidateResponse),
    Mutation(MutationResponse),
    MutationDryRun(MutationDryRunResponse),
    Commit(CommitResponse),
    CommitDryRun(CommitDryRunResponse),
}

impl CommandOutput {
    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(self).expect("command output should serialize")
    }

    pub fn to_text(&self) -> String {
        match self {
            CommandOutput::Scan(snapshot) => snapshot.to_text(),
            CommandOutput::Show(show) => show.to_text(),
            CommandOutput::Resolve(response) => response.to_text(),
            CommandOutput::Validate(response) => response.to_text(),
            CommandOutput::Mutation(response) => response.to_text(),
            CommandOutput::MutationDryRun(response) => response.to_text(),
            CommandOutput::Commit(response) => response.to_text(),
            CommandOutput::CommitDryRun(response) => response.to_text(),
        }
    }
}

impl Serialize for CommandOutput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            CommandOutput::Scan(snapshot) => snapshot.serialize(serializer),
            CommandOutput::Show(show) => show.serialize(serializer),
            CommandOutput::Resolve(response) => response.serialize(serializer),
            CommandOutput::Validate(response) => response.serialize(serializer),
            CommandOutput::Mutation(response) => response.serialize(serializer),
            CommandOutput::MutationDryRun(response) => response.serialize(serializer),
            CommandOutput::Commit(response) => response.serialize(serializer),
            CommandOutput::CommitDryRun(response) => response.serialize(serializer),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShowResponse {
    Hunk {
        snapshot_id: String,
        mode: cli::Mode,
        path: String,
        status: model::FileStatus,
        hunk: HunkView,
    },
    Change {
        snapshot_id: String,
        mode: cli::Mode,
        path: String,
        status: model::FileStatus,
        change: ChangeView,
    },
}

impl ShowResponse {
    fn to_text(&self) -> String {
        match self {
            ShowResponse::Hunk { path, hunk, .. } => {
                let mut out = format!("{} {}\n", path, hunk.id);
                out.push_str(&format!("{}\n", hunk.header));
                for line in &hunk.lines {
                    out.push_str(&format!("{}\n", render_numbered_line(line)));
                }
                out.trim_end().to_string()
            }
            ShowResponse::Change { path, change, .. } => {
                let mut out = format!("{} {}\n", path, change.id);
                out.push_str(&format!(
                    "{} ({}) [{} +{} -{} {}]\n",
                    change.header,
                    change.change_key,
                    change.metadata.kind.as_str(),
                    change.metadata.added_lines,
                    change.metadata.deleted_lines,
                    change.metadata.preview
                ));
                for line in &change.lines {
                    out.push_str(&format!("{}\n", render_numbered_line(line)));
                }
                out.trim_end().to_string()
            }
        }
    }
}

fn render_numbered_line(line: &model::DiffLineView) -> String {
    let old = line
        .old_lineno
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    let new = line
        .new_lineno
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!("{:>4} {:>4} {}", old, new, line.render())
}

#[derive(Debug, Serialize)]
pub struct MutationResponse {
    pub action: &'static str,
    pub snapshot_id: String,
    pub mode: cli::Mode,
    pub selected_hunks: Vec<String>,
    pub selected_changes: Vec<String>,
    pub selected_change_keys: Vec<String>,
    pub selected_line_ranges: Vec<String>,
    pub snapshot: SnapshotOutput,
}

impl MutationResponse {
    fn to_text(&self) -> String {
        format!(
            "{}d {} hunks, {} changes, {} change keys, and {} line ranges\nnext snapshot: {}",
            self.action,
            self.selected_hunks.len(),
            self.selected_changes.len(),
            self.selected_change_keys.len(),
            self.selected_line_ranges.len(),
            self.snapshot_id
        )
    }
}

#[derive(Debug, Serialize)]
pub struct MutationDryRunResponse {
    pub action: &'static str,
    pub dry_run: bool,
    pub snapshot_id: String,
    pub mode: cli::Mode,
    pub selected_hunks: Vec<String>,
    pub selected_changes: Vec<String>,
    pub selected_change_keys: Vec<String>,
    pub selected_line_ranges: Vec<String>,
    pub files: Vec<String>,
    pub patch: String,
    pub diffstat: String,
}

impl MutationDryRunResponse {
    fn to_text(&self) -> String {
        format!(
            "would {} {} files using {} hunks, {} changes, {} change keys, and {} line ranges\nsnapshot: {}",
            self.action,
            self.files.len(),
            self.selected_hunks.len(),
            self.selected_changes.len(),
            self.selected_change_keys.len(),
            self.selected_line_ranges.len(),
            self.snapshot_id
        )
    }
}

#[derive(Debug, Serialize)]
pub struct CommitResponse {
    pub commit: String,
    pub snapshot_id: String,
    pub selected_hunks: Vec<String>,
    pub selected_changes: Vec<String>,
    pub selected_change_keys: Vec<String>,
    pub selected_line_ranges: Vec<String>,
    pub snapshot: SnapshotOutput,
}

impl CommitResponse {
    fn to_text(&self) -> String {
        format!(
            "committed {} using {} hunks, {} changes, {} change keys, and {} line ranges\nnext snapshot: {}",
            self.commit,
            self.selected_hunks.len(),
            self.selected_changes.len(),
            self.selected_change_keys.len(),
            self.selected_line_ranges.len(),
            self.snapshot_id
        )
    }
}

#[derive(Debug, Serialize)]
pub struct CommitDryRunResponse {
    pub dry_run: bool,
    pub snapshot_id: String,
    pub messages: Vec<String>,
    pub selected_hunks: Vec<String>,
    pub selected_changes: Vec<String>,
    pub selected_change_keys: Vec<String>,
    pub selected_line_ranges: Vec<String>,
    pub files: Vec<String>,
    pub patch: String,
    pub diffstat: String,
}

impl CommitDryRunResponse {
    fn to_text(&self) -> String {
        format!(
            "would commit {} files using {} hunks, {} changes, {} change keys, and {} line ranges\nsnapshot: {}",
            self.files.len(),
            self.selected_hunks.len(),
            self.selected_changes.len(),
            self.selected_change_keys.len(),
            self.selected_line_ranges.len(),
            self.snapshot_id
        )
    }
}
