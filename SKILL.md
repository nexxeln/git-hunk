---
name: git-hunk
description: Non-interactive hunk and line-range staging with the `git-hunk` CLI. Use when a user wants atomic commits, selective staging, partial hunk staging, or an agent-safe replacement for `git add -p` or `git commit -p`, especially when `git-hunk` is available in the current repo or on PATH.
---

# Git Hunk

Use `git-hunk` to inspect, stage, unstage, and commit precise text changes without interactive prompts.

## Quick Start

1. Scan the repo and capture a snapshot:

```bash
git-hunk scan --mode stage --json
```

2. Inspect a selectable unit:

```bash
git-hunk show --mode stage <hunk-id>
git-hunk show --mode stage <change-id> --json
```

3. Apply a selection:

```bash
git-hunk stage --snapshot <snapshot-id> --hunk <hunk-id>
git-hunk stage --snapshot <snapshot-id> --change <change-id>
git-hunk stage --snapshot <snapshot-id> --hunk <hunk-id>:new:41-44
```

4. Commit the exact selection directly:

```bash
git-hunk commit -m "feat: message" --snapshot <snapshot-id> --change <change-id>
git-hunk commit -m "fix: message" --snapshot <snapshot-id> --hunk <hunk-id>:old:18-22
```

## Workflow

### Stage mode

- Use `scan --mode stage` for worktree changes relative to the index.
- Select by whole hunk, atomic change block, or line range.
- Prefer `--json` for agents; ids and `snapshot_id` come from scan output.

### Unstage mode

- Use `scan --mode unstage` for staged changes relative to `HEAD`.
- Use the same selectors with `unstage` to remove only part of the index.

```bash
git-hunk unstage --snapshot <snapshot-id> --change <change-id>
git-hunk unstage --snapshot <snapshot-id> --hunk <hunk-id>:old:10-12
```

### Line-range selectors

- Syntax: `<hunk-id>:<old|new>:<start-end>`.
- Use `new` when selecting added/replacement lines from stage mode.
- Use `old` when selecting the preimage side, especially in unstage mode.
- Use `show` without `--json` when you want numbered lines in terminal output.

## Snapshot Discipline

- Treat `snapshot_id` as mandatory for any mutating command.
- Rescan after every successful `stage`, `unstage`, or `commit`.
- If the command returns `stale_snapshot`, do not retry blindly; run `scan` again and use the fresh ids.

## Plan Files

Use a plan file when passing many selectors or when another tool is driving the workflow.

```json
{
  "snapshot_id": "s_123",
  "selectors": [
    { "type": "hunk", "id": "h_abc" },
    { "type": "change", "id": "c_def" },
    {
      "type": "line_range",
      "hunk_id": "h_xyz",
      "side": "new",
      "start": 41,
      "end": 44
    }
  ]
}
```

Run it with:

```bash
git-hunk stage --plan plan.json --json
git-hunk commit -m "refactor: split change" --plan plan.json --json
```

## Failure Handling

- If you get `ambiguous_line_range`, widen the range to cover the full atomic change or fall back to the `change_id` shown by `scan`.
- If a path appears under `unsupported`, do not try to force it through `git-hunk`; use normal git commands or a different workflow for conflicts, renames, copies, binary files, or non-UTF8 diffs.
- If there is nothing staged, `commit` fails unless `--allow-empty` is set.

## Practical Defaults

- Prefer `commit` with selectors when the user asked for a commit and you already know the exact changes.
- Prefer `stage` first when you need to inspect the staged result before committing.
- Keep commits atomic by scanning, selecting a minimal set, committing, then rescanning for the next commit.
