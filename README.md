# git-hunk

`git-hunk` is a small Rust CLI for non-interactive hunk staging.

It is meant for AI agents and other tooling that need an agent-safe replacement for `git add -p` so they can make atomic commits from Bash.

## Commands

- `scan` lists selectable hunks and change blocks and returns a `snapshot_id`
- `show` prints a hunk or change with line numbers
- `stage` stages selected hunks, changes, or line ranges
- `unstage` removes selected hunks, changes, or line ranges from the index
- `commit` stages a selection and commits it in one step

## Build

```bash
cargo build
```

## Example

```bash
git-hunk scan --mode stage --json
git-hunk show --mode stage <hunk-id>
git-hunk stage --snapshot <snapshot-id> --hunk <hunk-id>:new:41-44
git-hunk commit -m "feat: split atomic change" --snapshot <snapshot-id> --change <change-id>
```

## Notes

- Mutating commands require a fresh `snapshot_id`
- Rescan after every successful `stage`, `unstage`, or `commit`
- Unsupported paths like conflicts, renames, and binary diffs are reported instead of forced through

See `SKILL.md` for the agent workflow and selector syntax.
