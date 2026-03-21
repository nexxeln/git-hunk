# git-hunk

`git-hunk` is a small Rust CLI for non-interactive hunk staging.

It is meant for AI agents and other tooling that need an agent-safe replacement for `git add -p` so they can make atomic commits from Bash.

Recent agent-focused additions include:

- compact `scan` output with semantic metadata and short previews
- `commit --dry-run` using the real selection path without mutating the repo
- `resolve` for turning `file + line hint` into recommended selectors
- stable `change_key` identities that survive unrelated rescans
- structured error categories, retryability flags, and git command details

## Install

Install from crates.io after the first publish:

```bash
cargo install git-hunk
```

Install directly from GitHub at any time:

```bash
cargo install --git https://github.com/nexxeln/git-hunk.git
```

After the first GitHub release exists, install the latest binary with:

```bash
curl -fsSL https://raw.githubusercontent.com/nexxeln/git-hunk/main/install.sh | sh
```

Install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/nexxeln/git-hunk/main/install.sh | GIT_HUNK_VERSION=0.1.2 sh
```

The installer currently supports:

- macOS `aarch64`
- macOS `x86_64`
- Linux `x86_64`

## Commands

- `scan` lists selectable hunks and change blocks and returns a `snapshot_id`
- `show` prints a hunk, `change_id`, or `change_key` with line numbers
- `resolve` recommends `change_id`, `change_key`, and hunk selectors from a file+line hint
- `stage` stages selected hunks, `change_id`s, `change_key`s, or line ranges
- `unstage` removes selected hunks, `change_id`s, `change_key`s, or line ranges from the index
- `commit` stages a selection and commits it in one step, or previews it with `--dry-run`

## Build

```bash
cargo build
```

## Release

On every push to `main`, GitHub Actions runs tests, publishes a new crates.io version when `Cargo.toml` has an unpublished version, and creates a matching GitHub release with binary archives.

## Example

```bash
git-hunk scan --mode stage --compact --json
git-hunk resolve --mode stage --snapshot <snapshot-id> --path src/lib.rs --start 42 --json
git-hunk show --mode stage <change-key>
git-hunk commit -m "feat: split atomic change" --snapshot <snapshot-id> --change-key <change-key> --dry-run --json
git-hunk commit -m "feat: split atomic change" --snapshot <snapshot-id> --change-key <change-key>
```

## Notes

- Mutating commands require a fresh `snapshot_id`
- `change_key` is stable across unrelated rescans; `change_id` is snapshot-bound
- Rescan after every successful `stage`, `unstage`, or `commit`
- Unsupported paths like conflicts, renames, and binary diffs are reported instead of forced through

See `SKILL.md` for the agent workflow and selector syntax.
