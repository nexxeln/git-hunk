use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

const CLI_AFTER_HELP: &str = "Agent loop:\n  1. git-hunk scan --mode stage --compact --json\n  2. git-hunk resolve --mode stage --snapshot <snapshot-id> --path src/lib.rs --start 42 --json\n  3. git-hunk show --mode stage <change-key> --json\n  4. git-hunk stage --snapshot <snapshot-id> --change-key <change-key> --dry-run --json\n  5. git-hunk stage --snapshot <snapshot-id> --change-key <change-key> --json\n  6. git-hunk scan --mode stage --compact --json";

#[derive(Debug, Parser)]
#[command(name = "git-hunk")]
#[command(about = "Non-interactive hunk staging for AI agents")]
#[command(after_help = CLI_AFTER_HELP)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn json(&self) -> bool {
        match &self.command {
            Command::Scan(args) => args.json,
            Command::Show(args) => args.json,
            Command::Resolve(args) => args.json,
            Command::Validate(args) => args.json,
            Command::Stage(args) => args.json,
            Command::Unstage(args) => args.json,
            Command::Commit(args) => args.json,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Scan worktree or index changes into a snapshot")]
    Scan(ScanArgs),
    #[command(about = "Inspect a hunk, change id, or change key")]
    Show(ShowArgs),
    #[command(about = "Resolve a file and line hint into recommended selectors")]
    Resolve(ResolveArgs),
    #[command(about = "Validate selectors against the current snapshot and recover change keys")]
    Validate(ValidateArgs),
    #[command(about = "Stage an exact selection into the index")]
    Stage(MutateArgs),
    #[command(about = "Remove an exact selection from the index")]
    Unstage(MutateArgs),
    #[command(about = "Commit an exact selection, optionally with a dry-run preview")]
    Commit(CommitArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Stage,
    Unstage,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Stage => "stage",
            Mode::Unstage => "unstage",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ResolveSide {
    Auto,
    Old,
    New,
}

impl ResolveSide {
    pub fn as_str(self) -> &'static str {
        match self {
            ResolveSide::Auto => "auto",
            ResolveSide::Old => "old",
            ResolveSide::New => "new",
        }
    }
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    #[arg(
        long,
        value_enum,
        help = "Use 'stage' for worktree vs index or 'unstage' for index vs HEAD"
    )]
    pub mode: Mode,
    #[arg(long, help = "Return metadata without full diff lines")]
    pub compact: bool,
    #[arg(long, help = "Print structured JSON output")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    #[arg(
        long,
        value_enum,
        help = "Use 'stage' for worktree vs index or 'unstage' for index vs HEAD"
    )]
    pub mode: Mode,
    #[arg(help = "Hunk id, change id, or change key from scan")]
    pub id: String,
    #[arg(long, help = "Print structured JSON output")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ResolveArgs {
    #[arg(
        long,
        value_enum,
        help = "Use 'stage' for worktree vs index or 'unstage' for index vs HEAD"
    )]
    pub mode: Mode,
    #[arg(long, help = "Snapshot id from scan")]
    pub snapshot: String,
    #[arg(long, help = "Changed file path to resolve against")]
    pub path: String,
    #[arg(long, help = "Starting line number on the requested side")]
    pub start: u32,
    #[arg(long, help = "Optional ending line number; defaults to --start")]
    pub end: Option<u32>,
    #[arg(
        long,
        value_enum,
        default_value = "auto",
        help = "Prefer new lines, old lines, or auto-detect the diff side"
    )]
    pub side: ResolveSide,
    #[arg(long, help = "Print structured JSON output")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    #[arg(
        long,
        value_enum,
        help = "Use 'stage' for worktree vs index or 'unstage' for index vs HEAD"
    )]
    pub mode: Mode,
    #[arg(
        long,
        help = "Snapshot id to validate, or omit to validate selectors against the current state"
    )]
    pub snapshot: Option<String>,
    #[arg(
        long,
        help = "Selection plan JSON file, or '-' to read the plan from stdin"
    )]
    pub plan: Option<PathBuf>,
    #[arg(
        long = "hunk",
        help = "Whole hunk id or <hunk-id>:<old|new>:<start-end>"
    )]
    pub hunks: Vec<String>,
    #[arg(long = "change", help = "Snapshot-local change id from scan")]
    pub changes: Vec<String>,
    #[arg(
        long = "change-key",
        help = "Rescan-stable change key from scan or resolve"
    )]
    pub change_keys: Vec<String>,
    #[arg(long, help = "Return metadata without full diff lines")]
    pub compact: bool,
    #[arg(long, help = "Print structured JSON output")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct MutateArgs {
    #[arg(long, help = "Snapshot id from scan")]
    pub snapshot: Option<String>,
    #[arg(
        long,
        help = "Selection plan JSON file, or '-' to read the plan from stdin"
    )]
    pub plan: Option<PathBuf>,
    #[arg(
        long = "hunk",
        help = "Whole hunk id or <hunk-id>:<old|new>:<start-end>"
    )]
    pub hunks: Vec<String>,
    #[arg(long = "change", help = "Snapshot-local change id from scan")]
    pub changes: Vec<String>,
    #[arg(
        long = "change-key",
        help = "Rescan-stable change key from scan or resolve"
    )]
    pub change_keys: Vec<String>,
    #[arg(long, help = "Preview the staged result without mutating the repo")]
    pub dry_run: bool,
    #[arg(long, help = "Return the next snapshot without full diff lines")]
    pub compact: bool,
    #[arg(long, help = "Print structured JSON output")]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CommitArgs {
    #[arg(short = 'm', long = "message", required = true)]
    pub messages: Vec<String>,
    #[arg(long, help = "Snapshot id from scan")]
    pub snapshot: Option<String>,
    #[arg(
        long,
        help = "Selection plan JSON file, or '-' to read the plan from stdin"
    )]
    pub plan: Option<PathBuf>,
    #[arg(
        long = "hunk",
        help = "Whole hunk id or <hunk-id>:<old|new>:<start-end>"
    )]
    pub hunks: Vec<String>,
    #[arg(long = "change", help = "Snapshot-local change id from scan")]
    pub changes: Vec<String>,
    #[arg(
        long = "change-key",
        help = "Rescan-stable change key from scan or resolve"
    )]
    pub change_keys: Vec<String>,
    #[arg(long, help = "Allow an empty commit if nothing is staged")]
    pub allow_empty: bool,
    #[arg(long, help = "Preview the exact commit without mutating the repo")]
    pub dry_run: bool,
    #[arg(long, help = "Return the next snapshot without full diff lines")]
    pub compact: bool,
    #[arg(long, help = "Print structured JSON output")]
    pub json: bool,
}
