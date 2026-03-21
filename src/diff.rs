use crate::error::{AppError, AppResult};
use crate::model::{DiffLineView, FileStatus, LineKind, PatchLine};

#[derive(Debug, Clone)]
pub struct ParsedPatch {
    pub header_lines: Vec<String>,
    pub hunks: Vec<ParsedHunk>,
    pub status: FileStatus,
}

#[derive(Debug, Clone)]
pub struct ParsedHunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<PatchLine>,
}

pub fn parse_patch(text: &str) -> AppResult<ParsedPatch> {
    if text.trim().is_empty() {
        return Err(AppError::new(
            "empty_diff",
            "git produced an empty diff".to_string(),
        ));
    }

    let lines: Vec<&str> = text.lines().collect();

    if lines
        .iter()
        .any(|line| *line == "GIT binary patch" || line.starts_with("Binary files "))
    {
        return Err(AppError::new(
            "binary_file",
            "binary patches are not supported".to_string(),
        ));
    }

    let first_hunk = lines
        .iter()
        .position(|line| line.starts_with("@@ "))
        .ok_or_else(|| {
            AppError::new(
                "unsupported_diff",
                "diff does not contain text hunks".to_string(),
            )
        })?;

    let header_lines = lines[..first_hunk]
        .iter()
        .map(|line| (*line).to_string())
        .collect::<Vec<_>>();
    let status = if header_lines
        .iter()
        .any(|line| line.starts_with("new file mode "))
    {
        FileStatus::New
    } else if header_lines
        .iter()
        .any(|line| line.starts_with("deleted file mode "))
    {
        FileStatus::Deleted
    } else {
        FileStatus::Modified
    };

    let mut hunks = Vec::new();
    let mut index = first_hunk;
    while index < lines.len() {
        let header = lines[index];
        if !header.starts_with("@@ ") {
            return Err(AppError::new(
                "invalid_diff",
                format!("expected hunk header, got '{}'", header),
            ));
        }

        let (old_start, old_lines, new_start, new_lines) = parse_hunk_header(header)?;
        index += 1;
        let mut old_cursor = old_start;
        let mut new_cursor = new_start;
        let mut patch_lines = Vec::new();

        while index < lines.len() && !lines[index].starts_with("@@ ") {
            let line = lines[index];
            let parsed = match line.chars().next() {
                Some(' ') => {
                    let view = DiffLineView {
                        kind: LineKind::Context,
                        text: line[1..].to_string(),
                        old_lineno: Some(old_cursor),
                        new_lineno: Some(new_cursor),
                    };
                    old_cursor += 1;
                    new_cursor += 1;
                    PatchLine {
                        raw: line.to_string(),
                        view,
                    }
                }
                Some('-') => {
                    let view = DiffLineView {
                        kind: LineKind::Delete,
                        text: line[1..].to_string(),
                        old_lineno: Some(old_cursor),
                        new_lineno: None,
                    };
                    old_cursor += 1;
                    PatchLine {
                        raw: line.to_string(),
                        view,
                    }
                }
                Some('+') => {
                    let view = DiffLineView {
                        kind: LineKind::Add,
                        text: line[1..].to_string(),
                        old_lineno: None,
                        new_lineno: Some(new_cursor),
                    };
                    new_cursor += 1;
                    PatchLine {
                        raw: line.to_string(),
                        view,
                    }
                }
                Some('\\') => PatchLine {
                    raw: line.to_string(),
                    view: DiffLineView {
                        kind: LineKind::Note,
                        text: line.trim_start_matches("\\ ").to_string(),
                        old_lineno: None,
                        new_lineno: None,
                    },
                },
                _ => {
                    return Err(AppError::new(
                        "invalid_diff",
                        format!("unsupported diff line '{}'", line),
                    ));
                }
            };
            patch_lines.push(parsed);
            index += 1;
        }

        hunks.push(ParsedHunk {
            header: header.to_string(),
            old_start,
            old_lines,
            new_start,
            new_lines,
            lines: patch_lines,
        });
    }

    Ok(ParsedPatch {
        header_lines,
        hunks,
        status,
    })
}

fn parse_hunk_header(header: &str) -> AppResult<(u32, u32, u32, u32)> {
    let rest = header.trim_start_matches("@@ ");
    let end = rest.find(" @@").ok_or_else(|| {
        AppError::new("invalid_diff", format!("invalid hunk header '{}'", header))
    })?;
    let content = &rest[..end];
    let mut pieces = content.split_whitespace();
    let old = pieces.next().ok_or_else(|| {
        AppError::new("invalid_diff", format!("missing old range in '{}'", header))
    })?;
    let new = pieces.next().ok_or_else(|| {
        AppError::new("invalid_diff", format!("missing new range in '{}'", header))
    })?;

    let (old_start, old_lines) = parse_range(old, '-')?;
    let (new_start, new_lines) = parse_range(new, '+')?;
    Ok((old_start, old_lines, new_start, new_lines))
}

fn parse_range(token: &str, prefix: char) -> AppResult<(u32, u32)> {
    let token = token.strip_prefix(prefix).ok_or_else(|| {
        AppError::new(
            "invalid_diff",
            format!("range '{}' does not start with '{}'", token, prefix),
        )
    })?;

    let mut parts = token.split(',');
    let start = parts
        .next()
        .ok_or_else(|| {
            AppError::new(
                "invalid_diff",
                format!("missing range start in '{}'", token),
            )
        })?
        .parse::<u32>()
        .map_err(|_| {
            AppError::new(
                "invalid_diff",
                format!("invalid range start in '{}'", token),
            )
        })?;
    let count = parts
        .next()
        .map(|part| {
            part.parse::<u32>().map_err(|_| {
                AppError::new(
                    "invalid_diff",
                    format!("invalid range count in '{}'", token),
                )
            })
        })
        .transpose()?
        .unwrap_or(1);

    Ok((start, count))
}
