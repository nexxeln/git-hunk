pub mod cli;
mod diff;
mod error;
mod git;
mod model;
mod patch;
mod scan;
mod select;

use std::path::PathBuf;

use cli::{Cli, Command, CommitArgs, MutateArgs, ScanArgs, ShowArgs};
use error::{AppError, AppResult};
use model::{ChangeView, HunkView, ScanState, SelectionPlan, SnapshotView};
use select::SelectionInput;
use serde::Serialize;

pub use error::AppError as Error;

pub fn run(cli: Cli) -> AppResult<CommandOutput> {
    let repo_root = git::repo_root(&std::env::current_dir().map_err(AppError::io)?)?;

    match cli.command {
        Command::Scan(args) => scan_command(&repo_root, args),
        Command::Show(args) => show_command(&repo_root, args),
        Command::Stage(args) => mutate_command(&repo_root, args, false),
        Command::Unstage(args) => mutate_command(&repo_root, args, true),
        Command::Commit(args) => commit_command(&repo_root, args),
    }
}

fn scan_command(repo_root: &PathBuf, args: ScanArgs) -> AppResult<CommandOutput> {
    let state = scan::scan_repo(repo_root, args.mode)?;
    Ok(CommandOutput::Scan(state.snapshot.clone()))
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

    Err(AppError::new(
        "unknown_id",
        format!("no hunk or change found for id '{}'", args.id),
    ))
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
    let selection = load_selection_input(args.snapshot, args.plan, args.hunks, args.changes)?;
    let state = validate_snapshot(repo_root, mode, &selection)?;
    let resolved = select::resolve_selection(&state, &selection)?;
    let patch = patch::build_patch(&state, &resolved)?;

    git::apply_patch(repo_root, &patch, reverse)?;

    let next_state = scan::scan_repo(repo_root, mode)?;
    Ok(CommandOutput::Mutation(MutationResponse {
        action: if reverse { "unstage" } else { "stage" },
        snapshot_id: next_state.snapshot.snapshot_id.clone(),
        mode,
        selected_hunks: resolved.selected_hunks,
        selected_changes: resolved.selected_changes,
        snapshot: next_state.snapshot,
    }))
}

fn commit_command(repo_root: &PathBuf, args: CommitArgs) -> AppResult<CommandOutput> {
    if args.messages.is_empty() {
        return Err(AppError::new(
            "missing_message",
            "commit requires at least one message".to_string(),
        ));
    }

    let selection = load_selection_input(args.snapshot, args.plan, args.hunks, args.changes)?;
    let mut selected_hunks = Vec::new();
    let mut selected_changes = Vec::new();

    if selection.has_selectors() {
        let state = validate_snapshot(repo_root, cli::Mode::Stage, &selection)?;
        let resolved = select::resolve_selection(&state, &selection)?;
        let patch = patch::build_patch(&state, &resolved)?;
        git::apply_patch(repo_root, &patch, false)?;
        selected_hunks = resolved.selected_hunks;
        selected_changes = resolved.selected_changes;
    } else if let Some(snapshot_id) = selection.snapshot_id.as_ref() {
        let state = scan::scan_repo(repo_root, cli::Mode::Stage)?;
        if state.snapshot.snapshot_id != *snapshot_id {
            return Err(AppError::new(
                "stale_snapshot",
                format!(
                    "snapshot '{}' no longer matches the current stage view '{}'",
                    snapshot_id, state.snapshot.snapshot_id
                ),
            ));
        }
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
        selected_hunks,
        selected_changes,
        snapshot: next_state.snapshot,
    }))
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
        return Err(AppError::new(
            "stale_snapshot",
            format!(
                "snapshot '{}' no longer matches the current {} view '{}'",
                snapshot_id,
                mode.as_str(),
                state.snapshot.snapshot_id
            ),
        ));
    }
    Ok(state)
}

fn load_selection_input(
    snapshot: Option<String>,
    plan_path: Option<PathBuf>,
    hunks: Vec<String>,
    changes: Vec<String>,
) -> AppResult<SelectionInput> {
    let mut input = SelectionInput {
        snapshot_id: snapshot,
        hunk_ids: hunks,
        change_ids: changes,
    };

    if let Some(path) = plan_path {
        let contents = std::fs::read_to_string(&path).map_err(|err| {
            AppError::new(
                "plan_read_failed",
                format!("failed to read {}: {}", path.display(), err),
            )
        })?;
        let plan: SelectionPlan = serde_json::from_str(&contents).map_err(|err| {
            AppError::new(
                "plan_parse_failed",
                format!("failed to parse {}: {}", path.display(), err),
            )
        })?;

        if input.snapshot_id.is_none() {
            input.snapshot_id = Some(plan.snapshot_id);
        }
        for selector in plan.selectors {
            match selector {
                model::PlanSelector::Hunk { id } => input.hunk_ids.push(id),
                model::PlanSelector::Change { id } => input.change_ids.push(id),
            }
        }
    }

    Ok(input)
}

#[derive(Debug)]
pub enum CommandOutput {
    Scan(SnapshotView),
    Show(ShowResponse),
    Mutation(MutationResponse),
    Commit(CommitResponse),
}

impl CommandOutput {
    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(self).expect("command output should serialize")
    }

    pub fn to_text(&self) -> String {
        match self {
            CommandOutput::Scan(snapshot) => snapshot.to_text(),
            CommandOutput::Show(show) => show.to_text(),
            CommandOutput::Mutation(response) => response.to_text(),
            CommandOutput::Commit(response) => response.to_text(),
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
            CommandOutput::Mutation(response) => response.serialize(serializer),
            CommandOutput::Commit(response) => response.serialize(serializer),
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
                    out.push_str(&format!("{}\n", line.render()));
                }
                out.trim_end().to_string()
            }
            ShowResponse::Change { path, change, .. } => {
                let mut out = format!("{} {}\n", path, change.id);
                out.push_str(&format!("{}\n", change.header));
                for line in &change.lines {
                    out.push_str(&format!("{}\n", line.render()));
                }
                out.trim_end().to_string()
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MutationResponse {
    pub action: &'static str,
    pub snapshot_id: String,
    pub mode: cli::Mode,
    pub selected_hunks: Vec<String>,
    pub selected_changes: Vec<String>,
    pub snapshot: SnapshotView,
}

impl MutationResponse {
    fn to_text(&self) -> String {
        format!(
            "{}d {} hunks and {} changes\nnext snapshot: {}",
            self.action,
            self.selected_hunks.len(),
            self.selected_changes.len(),
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
    pub snapshot: SnapshotView,
}

impl CommitResponse {
    fn to_text(&self) -> String {
        format!(
            "committed {}\nnext snapshot: {}",
            self.commit, self.snapshot_id
        )
    }
}
