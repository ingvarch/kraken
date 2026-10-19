# kraken

Release a project in one command: bump the version, commit, tag, push.

```
$ kraken
Version files: package.json
Current version: 0.5.0
? Next version ›
❯ patch  0.5.1
  minor  0.6.0
  major  1.0.0
Updated package.json, package-lock.json
Committed "chore(release): v0.5.1"
Tagged v0.5.1
? Push main and v0.5.1 to origin? (y/n) › yes
Pushed main and v0.5.1 to origin
```

## What it does

1. Checks that you are in a git repository, on a branch, with no uncommitted changes to tracked files. A shallow clone is refused because it may miss tags. If there is an `origin` remote, kraken asks it for its tags and refuses to go on while origin has release tags you have not fetched. If origin cannot be reached, kraken warns and goes on.
2. Looks for version files in the repository root:

   | File | Version field |
   |---|---|
   | `package.json` | `version` |
   | `Cargo.toml` | `[package].version` or `[workspace.package].version` |
   | `pyproject.toml` | `[project].version` or `[tool.poetry].version` |

   If no file carries a version (a Go module, a `dynamic` Python version, anything else), kraken works with git tags only.
3. Takes the current version from those files, or from the highest `X.Y.Z` / `vX.Y.Z` tag reachable from HEAD. Warns when the files and the latest tag disagree, and refuses a new version that is not higher than the latest tag.
4. Asks for patch, minor or major.
5. Rewrites only the version string; comments, quotes and formatting stay as they were. Sets the project's own entry in the lock file, if there is one: `package-lock.json`, `Cargo.lock` or `uv.lock`. Nothing else in the lock file changes, and no network is needed. For Cargo, kraken reads the workspace members with `cargo metadata --no-deps` and refuses a version that a path dependency inside the workspace would not accept.
6. Commits `chore(release): v1.2.3` and creates an annotated tag. The tag keeps the style of the latest tag (`v1.2.3` or `1.2.3`); the default is `v`. A lock file that git does not track is updated but left out of the commit. With no version files there is nothing to commit, so kraken only tags HEAD.
7. If there is an `origin` remote, asks whether to push the branch and the tag. The branch goes to its upstream branch on origin, or to a branch with the same name. Both go in one atomic push.

If anything fails before the tag exists, kraken moves HEAD back and puts the files back as they were.

## Usage

```
kraken [patch|minor|major] [--yes] [--dry-run]
```

- `kraken` asks for the level and for the push.
- `kraken minor` skips the first question.
- `kraken minor --yes` asks nothing and pushes if `origin` exists. Use this in scripts.
- `kraken --dry-run` shows what would change and changes nothing.

## Install

```
cargo install --path .
```

Works on macOS and Linux.

## Limits

- One version for the whole repository. A Cargo workspace needs `[workspace.package].version`; per-crate versions are not supported.
- kraken does not change dependency requirements. If `app` depends on `engine = { path = "../engine", version = "0.1.0" }`, a bump to 0.2.0 is refused until you update the requirement.
- Only `X.Y.Z` versions. Pre-release (`1.0.0-rc.1`) and build metadata are rejected; pre-release tags are ignored.
- No changelog and no GitHub releases.

## Development

```
cargo test
```

The tests create temporary git repositories and need `git` and `cargo` on `PATH`.
