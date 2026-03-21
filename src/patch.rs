use crate::error::{AppError, AppResult};
use crate::model::ScanState;
use crate::select::ResolvedSelection;

pub fn build_patch(state: &ScanState, selection: &ResolvedSelection) -> AppResult<String> {
    let mut out = String::new();

    for (file_index, change_indexes) in &selection.per_file_change_indexes {
        let file = &state.files[*file_index];
        if change_indexes.is_empty() {
            continue;
        }

        if file.patch_header_lines.is_empty() {
            return Err(AppError::new(
                "invalid_patch",
                format!("file '{}' is missing patch headers", file.path),
            ));
        }

        for line in &file.patch_header_lines {
            out.push_str(line);
            out.push('\n');
        }
        for change_index in change_indexes {
            let change = &file.changes[*change_index];
            out.push_str(&change.header);
            out.push('\n');
            for line in &change.lines {
                out.push_str(&line.raw);
                out.push('\n');
            }
        }
    }

    if out.is_empty() {
        return Err(AppError::new(
            "empty_patch",
            "selection did not produce a patch".to_string(),
        ));
    }

    Ok(out)
}
