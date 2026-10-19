# kraken

Release a project in one command: bump the version, commit, tag, push.

```
╭ kraken ──────────────────────────────────────────────────╮
│                                                          │
│  Version   1.4.2 from package.json                       │
│  Branch    main → origin/main                            │
│                                                          │
│  Next version                                            │
│  ❯ patch   1.4.3   bug fixes                             │
│    minor   1.5.0   new features                          │
│    major   2.0.0   breaking changes                      │
│                                                          │
├──────────────────────────────────────────────────────────┤
│  ↑↓ choose   enter continue   esc quit                   │
╰──────────────────────────────────────────────────────────╯
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

- `kraken` opens a full-screen panel: pick the level with the arrow keys (or `j`/`k`), see what the release will change, turn the push on or off with space, and release with enter. `esc` goes back, `q` quits. On enter the panel closes and kraken writes, commits, tags and pushes in the terminal, so git can ask for a passphrase or credentials there.
- `kraken minor` opens the panel on the confirmation of a minor release.
- `kraken minor --yes` opens no panel, asks nothing and pushes if `origin` exists. Use this in scripts.
- `kraken minor --dry-run` shows what would change and changes nothing.

Without a terminal (CI, pipes), kraken needs the level and `--yes`.

## Install

```
brew install ingvarch/tap/kraken
```

Or download an archive for your platform from the [releases](https://github.com/ingvarch/kraken/releases), or build from source with `cargo install --path .`. kraken works on macOS and Linux.

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

## Releasing kraken

Release kraken with kraken: `kraken` in this repository bumps `Cargo.toml`, tags and pushes. The tag starts the `release` workflow, which builds the archives with GoReleaser, signs and notarizes the macOS binaries and updates the cask in [ingvarch/homebrew-tap](https://github.com/ingvarch/homebrew-tap). The workflow stops if the tag and the version in `Cargo.toml` differ.

## License

MIT. The archives include `THIRD_PARTY_NOTICES` with the licenses of the crates kraken links.
