use serde::{Deserialize, Serialize};

use crate::cli::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Modified,
    New,
    Deleted,
}

impl FileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            FileStatus::Modified => "modified",
            FileStatus::New => "new",
            FileStatus::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Context,
    Add,
    Delete,
    Note,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffLineView {
    pub kind: LineKind,
    pub text: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

impl DiffLineView {
    pub fn render(&self) -> String {
        match self.kind {
            LineKind::Context => format!(" {}", self.text),
            LineKind::Add => format!("+{}", self.text),
            LineKind::Delete => format!("-{}", self.text),
            LineKind::Note => format!("\\ {}", self.text),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeView {
    pub id: String,
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLineView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HunkView {
    pub id: String,
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLineView>,
    pub changes: Vec<ChangeView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileView {
    pub path: String,
    pub status: FileStatus,
    pub hunks: Vec<HunkView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnsupportedPath {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotView {
    pub snapshot_id: String,
    pub mode: Mode,
    pub files: Vec<FileView>,
    pub unsupported: Vec<UnsupportedPath>,
}

impl SnapshotView {
    pub fn to_text(&self) -> String {
        let mut out = format!("snapshot: {} ({})", self.snapshot_id, self.mode.as_str());
        for file in &self.files {
            out.push_str(&format!("\n{} [{}]", file.path, file.status.as_str()));
            for hunk in &file.hunks {
                out.push_str(&format!("\n  {} {}", hunk.id, hunk.header));
                for change in &hunk.changes {
                    out.push_str(&format!("\n    {} {}", change.id, change.header));
                }
            }
        }
        if !self.unsupported.is_empty() {
            out.push_str("\nunsupported:");
            for item in &self.unsupported {
                out.push_str(&format!("\n  {} ({})", item.path, item.reason));
            }
        }
        out
    }
}

#[derive(Debug, Deserialize)]
pub struct SelectionPlan {
    pub snapshot_id: String,
    pub selectors: Vec<PlanSelector>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlanSelector {
    Hunk { id: String },
    Change { id: String },
}

#[derive(Debug, Clone)]
pub struct PatchLine {
    pub raw: String,
    pub view: DiffLineView,
}

#[derive(Debug, Clone)]
pub struct HunkState {
    pub id: String,
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<PatchLine>,
    pub change_indexes: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct ChangeState {
    pub id: String,
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<PatchLine>,
}

#[derive(Debug, Clone)]
pub struct FileState {
    pub path: String,
    pub status: FileStatus,
    pub patch_header_lines: Vec<String>,
    pub hunks: Vec<HunkState>,
    pub changes: Vec<ChangeState>,
}

#[derive(Debug, Clone)]
pub struct ScanState {
    pub snapshot: SnapshotView,
    pub files: Vec<FileState>,
}

impl ScanState {
    pub fn find_hunk(&self, id: &str) -> Option<(&FileView, &HunkView)> {
        self.snapshot.files.iter().find_map(|file| {
            file.hunks
                .iter()
                .find(|hunk| hunk.id == id)
                .map(|hunk| (file, hunk))
        })
    }

    pub fn find_change(&self, id: &str) -> Option<(&FileView, &ChangeView)> {
        self.snapshot.files.iter().find_map(|file| {
            file.hunks.iter().find_map(|hunk| {
                hunk.changes
                    .iter()
                    .find(|change| change.id == id)
                    .map(|change| (file, change))
            })
        })
    }
}
