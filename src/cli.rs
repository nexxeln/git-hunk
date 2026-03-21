use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(name = "git-hunk")]
#[command(about = "Non-interactive hunk staging for AI agents")]
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
            Command::Stage(args) => args.json,
            Command::Unstage(args) => args.json,
            Command::Commit(args) => args.json,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Scan(ScanArgs),
    Show(ShowArgs),
    Resolve(ResolveArgs),
    Stage(MutateArgs),
    Unstage(MutateArgs),
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
    #[arg(long, value_enum)]
    pub mode: Mode,
    #[arg(long)]
    pub compact: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    #[arg(long, value_enum)]
    pub mode: Mode,
    pub id: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ResolveArgs {
    #[arg(long, value_enum)]
    pub mode: Mode,
    #[arg(long)]
    pub snapshot: String,
    #[arg(long)]
    pub path: String,
    #[arg(long)]
    pub start: u32,
    #[arg(long)]
    pub end: Option<u32>,
    #[arg(long, value_enum, default_value = "auto")]
    pub side: ResolveSide,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct MutateArgs {
    #[arg(long)]
    pub snapshot: Option<String>,
    #[arg(long)]
    pub plan: Option<PathBuf>,
    #[arg(long = "hunk")]
    pub hunks: Vec<String>,
    #[arg(long = "change")]
    pub changes: Vec<String>,
    #[arg(long = "change-key")]
    pub change_keys: Vec<String>,
    #[arg(long)]
    pub compact: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CommitArgs {
    #[arg(short = 'm', long = "message", required = true)]
    pub messages: Vec<String>,
    #[arg(long)]
    pub snapshot: Option<String>,
    #[arg(long)]
    pub plan: Option<PathBuf>,
    #[arg(long = "hunk")]
    pub hunks: Vec<String>,
    #[arg(long = "change")]
    pub changes: Vec<String>,
    #[arg(long = "change-key")]
    pub change_keys: Vec<String>,
    #[arg(long)]
    pub allow_empty: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub compact: bool,
    #[arg(long)]
    pub json: bool,
}
