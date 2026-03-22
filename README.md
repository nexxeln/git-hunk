# git-hunk

`git-hunk` is a small Rust CLI for non-interactive hunk staging.

It lets you scan diffs, inspect exact changes, stage or unstage precise selections, and preview commits without using `git add -p`.

## Install

From crates.io:

```bash
cargo install git-hunk
```

From GitHub:

```bash
cargo install --git https://github.com/nexxeln/git-hunk.git
```

Binary installer:

```bash
curl -fsSL https://raw.githubusercontent.com/nexxeln/git-hunk/main/install.sh | sh
```

## Commands

- `scan` lists selectable hunks and changes and returns a `snapshot_id`
- `show` prints a hunk, `change_id`, or `change_key`
- `resolve` maps a file and line hint to matching selectors
- `validate` checks whether selectors still match the current snapshot
- `stage` stages selected hunks, changes, change keys, or line ranges
- `unstage` removes selected hunks, changes, change keys, or line ranges from the index
- `commit` stages a selection and commits it, or previews it with `--dry-run`

## Example

```bash
git-hunk scan --mode stage --compact --json
git-hunk show --mode stage <change-key> --json
git-hunk stage --snapshot <snapshot-id> --change-key <change-key> --dry-run --json
git-hunk stage --snapshot <snapshot-id> --change-key <change-key> --json
```

## Notes

- Mutating commands require a fresh `snapshot_id`
- `change_id` is snapshot-bound; `change_key` is more stable across rescans
- Unsupported paths like conflicts, renames, and binary diffs are reported instead of forced through

## Build

```bash
cargo build
```

## Release

On every push to `main`, GitHub Actions runs tests, publishes a new crates.io version when `Cargo.toml` has an unpublished version, and creates a matching GitHub release with binary archives.
