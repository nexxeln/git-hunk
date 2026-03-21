use serde::Serialize;
use serde_json::Value;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Snapshot,
    Selector,
    Unsupported,
    Git,
    Io,
    Parse,
    State,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
    pub category: ErrorCategory,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl AppError {
    pub fn new(code: &'static str, message: String) -> Self {
        let (category, retryable) = classify(code);
        Self {
            code,
            message,
            category,
            retryable,
            details: None,
        }
    }

    pub fn io(err: std::io::Error) -> Self {
        Self {
            code: "io_error",
            message: err.to_string(),
            category: ErrorCategory::Io,
            retryable: false,
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn to_json_string(&self) -> String {
        serde_json::to_string_pretty(&ErrorEnvelope {
            error: self.clone(),
        })
        .expect("error should serialize")
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;

#[derive(Serialize)]
struct ErrorEnvelope {
    error: AppError,
}

fn classify(code: &str) -> (ErrorCategory, bool) {
    match code {
        "missing_snapshot" | "stale_snapshot" => (ErrorCategory::Snapshot, true),
        "invalid_hunk_selector"
        | "invalid_resolve_range"
        | "missing_selection"
        | "unknown_hunk"
        | "unknown_change"
        | "unknown_id"
        | "unknown_path"
        | "no_changes_in_path"
        | "no_resolve_candidates"
        | "ambiguous_line_range"
        | "empty_line_range" => (ErrorCategory::Selector, false),
        "binary_file" | "unsupported_diff" | "empty_diff" | "non_utf8_diff" => {
            (ErrorCategory::Unsupported, false)
        }
        "git_repo_root_failed"
        | "git_inventory_failed"
        | "git_diff_failed"
        | "git_diff_check_failed"
        | "git_apply_check_failed"
        | "git_apply_failed"
        | "git_commit_failed"
        | "git_rev_parse_failed"
        | "git_index_path_failed"
        | "git_read_tree_failed"
        | "git_diff_name_only_failed"
        | "git_command_failed" => (ErrorCategory::Git, false),
        "io_error" | "file_read_failed" | "plan_read_failed" => (ErrorCategory::Io, false),
        "plan_parse_failed" | "invalid_diff" | "mapping_failed" => (ErrorCategory::Parse, false),
        _ => (ErrorCategory::State, false),
    }
}
