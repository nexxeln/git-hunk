use serde::{Deserialize, Serialize};

use crate::cli::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineSide {
    Old,
    New,
}

impl LineSide {
    pub fn as_str(self) -> &'static str {
        match self {
            LineSide::Old => "old",
            LineSide::New => "new",
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Addition,
    Deletion,
    Replacement,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Addition => "addition",
            ChangeKind::Deletion => "deletion",
            ChangeKind::Replacement => "replacement",
        }
    }
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
pub struct ChangeMetadata {
    pub kind: ChangeKind,
    pub added_lines: u32,
    pub deleted_lines: u32,
    pub whitespace_only: bool,
    pub preview: String,
}

impl ChangeMetadata {
    pub fn from_lines(lines: &[DiffLineView]) -> Self {
        let added = lines
            .iter()
            .filter(|line| line.kind == LineKind::Add)
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        let deleted = lines
            .iter()
            .filter(|line| line.kind == LineKind::Delete)
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();

        let kind = match (added.is_empty(), deleted.is_empty()) {
            (false, true) => ChangeKind::Addition,
            (true, false) => ChangeKind::Deletion,
            (false, false) => ChangeKind::Replacement,
            (true, true) => ChangeKind::Replacement,
        };

        let whitespace_only = if added.is_empty() && deleted.is_empty() {
            false
        } else {
            normalize_lines(&added) == normalize_lines(&deleted)
        };

        let preview = preview_lines(lines);

        Self {
            kind,
            added_lines: added.len() as u32,
            deleted_lines: deleted.len() as u32,
            whitespace_only,
            preview,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeView {
    pub id: String,
    pub change_key: String,
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub metadata: ChangeMetadata,
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
                    out.push_str(&format!(
                        "\n    {} {} {} +{} -{} {}",
                        change.id,
                        change.change_key,
                        change.metadata.kind.as_str(),
                        change.metadata.added_lines,
                        change.metadata.deleted_lines,
                        change.metadata.preview
                    ));
                }
            }
        }
        append_unsupported(&mut out, &self.unsupported);
        out
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactChangeView {
    pub id: String,
    pub change_key: String,
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub metadata: ChangeMetadata,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactHunkView {
    pub id: String,
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub changes: Vec<CompactChangeView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactFileView {
    pub path: String,
    pub status: FileStatus,
    pub hunks: Vec<CompactHunkView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactSnapshotView {
    pub snapshot_id: String,
    pub mode: Mode,
    pub files: Vec<CompactFileView>,
    pub unsupported: Vec<UnsupportedPath>,
}

impl CompactSnapshotView {
    pub fn to_text(&self) -> String {
        let mut out = format!("snapshot: {} ({})", self.snapshot_id, self.mode.as_str());
        for file in &self.files {
            out.push_str(&format!("\n{} [{}]", file.path, file.status.as_str()));
            for hunk in &file.hunks {
                out.push_str(&format!("\n  {} {}", hunk.id, hunk.header));
                for change in &hunk.changes {
                    out.push_str(&format!(
                        "\n    {} {} {} +{} -{} {}",
                        change.id,
                        change.change_key,
                        change.metadata.kind.as_str(),
                        change.metadata.added_lines,
                        change.metadata.deleted_lines,
                        change.metadata.preview
                    ));
                }
            }
        }
        append_unsupported(&mut out, &self.unsupported);
        out
    }
}

impl From<&SnapshotView> for CompactSnapshotView {
    fn from(snapshot: &SnapshotView) -> Self {
        Self {
            snapshot_id: snapshot.snapshot_id.clone(),
            mode: snapshot.mode,
            files: snapshot
                .files
                .iter()
                .map(|file| CompactFileView {
                    path: file.path.clone(),
                    status: file.status,
                    hunks: file
                        .hunks
                        .iter()
                        .map(|hunk| CompactHunkView {
                            id: hunk.id.clone(),
                            header: hunk.header.clone(),
                            old_start: hunk.old_start,
                            old_lines: hunk.old_lines,
                            new_start: hunk.new_start,
                            new_lines: hunk.new_lines,
                            changes: hunk
                                .changes
                                .iter()
                                .map(|change| CompactChangeView {
                                    id: change.id.clone(),
                                    change_key: change.change_key.clone(),
                                    header: change.header.clone(),
                                    old_start: change.old_start,
                                    old_lines: change.old_lines,
                                    new_start: change.new_start,
                                    new_lines: change.new_lines,
                                    metadata: change.metadata.clone(),
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .collect(),
            unsupported: snapshot.unsupported.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SnapshotOutput {
    Full(SnapshotView),
    Compact(CompactSnapshotView),
}

impl SnapshotOutput {
    pub fn from_snapshot(snapshot: SnapshotView, compact: bool) -> Self {
        if compact {
            Self::Compact(CompactSnapshotView::from(&snapshot))
        } else {
            Self::Full(snapshot)
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            SnapshotOutput::Full(snapshot) => snapshot.to_text(),
            SnapshotOutput::Compact(snapshot) => snapshot.to_text(),
        }
    }

    pub fn snapshot_id(&self) -> &str {
        match self {
            SnapshotOutput::Full(snapshot) => &snapshot.snapshot_id,
            SnapshotOutput::Compact(snapshot) => &snapshot.snapshot_id,
        }
    }
}

impl Serialize for SnapshotOutput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            SnapshotOutput::Full(snapshot) => snapshot.serialize(serializer),
            SnapshotOutput::Compact(snapshot) => snapshot.serialize(serializer),
        }
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
    Hunk {
        id: String,
    },
    Change {
        id: String,
    },
    ChangeKey {
        key: String,
    },
    LineRange {
        hunk_id: String,
        side: LineSide,
        start: u32,
        end: u32,
    },
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
    pub change_key: String,
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

    pub fn find_change_key(&self, key: &str) -> Option<(&FileView, &ChangeView)> {
        self.snapshot.files.iter().find_map(|file| {
            file.hunks.iter().find_map(|hunk| {
                hunk.changes
                    .iter()
                    .find(|change| change.change_key == key)
                    .map(|change| (file, change))
            })
        })
    }
}

fn append_unsupported(out: &mut String, unsupported: &[UnsupportedPath]) {
    if !unsupported.is_empty() {
        out.push_str("\nunsupported:");
        for item in unsupported {
            out.push_str(&format!("\n  {} ({})", item.path, item.reason));
        }
    }
}

fn normalize_lines(lines: &[&str]) -> String {
    lines
        .iter()
        .flat_map(|line| line.chars().filter(|ch| !ch.is_whitespace()))
        .collect()
}

fn preview_lines(lines: &[DiffLineView]) -> String {
    let mut preview = lines
        .iter()
        .filter_map(|line| match line.kind {
            LineKind::Add => Some(preview_fragment('+', &line.text)),
            LineKind::Delete => Some(preview_fragment('-', &line.text)),
            _ => None,
        })
        .take(2)
        .collect::<Vec<_>>()
        .join(" | ");

    if preview.is_empty() {
        preview = "(no changed lines)".to_string();
    }

    if preview.chars().count() > 80 {
        preview = preview.chars().take(77).collect::<String>() + "...";
    }

    preview
}

fn preview_fragment(prefix: char, text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        format!("{}<blank>", prefix)
    } else {
        format!("{}{}", prefix, trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::{ChangeKind, ChangeMetadata, DiffLineView, LineKind};

    #[test]
    fn metadata_detects_additions() {
        let metadata = ChangeMetadata::from_lines(&[DiffLineView {
            kind: LineKind::Add,
            text: "hello".to_string(),
            old_lineno: None,
            new_lineno: Some(1),
        }]);

        assert_eq!(metadata.kind, ChangeKind::Addition);
        assert_eq!(metadata.added_lines, 1);
        assert_eq!(metadata.deleted_lines, 0);
        assert!(!metadata.whitespace_only);
        assert_eq!(metadata.preview, "+hello");
    }

    #[test]
    fn metadata_detects_replacements() {
        let metadata = ChangeMetadata::from_lines(&[
            DiffLineView {
                kind: LineKind::Delete,
                text: "old".to_string(),
                old_lineno: Some(1),
                new_lineno: None,
            },
            DiffLineView {
                kind: LineKind::Add,
                text: "new".to_string(),
                old_lineno: None,
                new_lineno: Some(1),
            },
        ]);

        assert_eq!(metadata.kind, ChangeKind::Replacement);
        assert_eq!(metadata.added_lines, 1);
        assert_eq!(metadata.deleted_lines, 1);
        assert!(!metadata.whitespace_only);
    }

    #[test]
    fn metadata_detects_whitespace_only_changes() {
        let metadata = ChangeMetadata::from_lines(&[
            DiffLineView {
                kind: LineKind::Delete,
                text: "    let value = 1;".to_string(),
                old_lineno: Some(1),
                new_lineno: None,
            },
            DiffLineView {
                kind: LineKind::Add,
                text: "  let value = 1;".to_string(),
                old_lineno: None,
                new_lineno: Some(1),
            },
        ]);

        assert!(metadata.whitespace_only);
    }

    #[test]
    fn metadata_preview_marks_blank_lines() {
        let metadata = ChangeMetadata::from_lines(&[
            DiffLineView {
                kind: LineKind::Delete,
                text: String::new(),
                old_lineno: Some(1),
                new_lineno: None,
            },
            DiffLineView {
                kind: LineKind::Add,
                text: String::new(),
                old_lineno: None,
                new_lineno: Some(1),
            },
        ]);

        assert_eq!(metadata.preview, "-<blank> | +<blank>");
    }
}
