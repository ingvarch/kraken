# kraken

[![CI](https://github.com/ingvarch/kraken/actions/workflows/ci.yml/badge.svg)](https://github.com/ingvarch/kraken/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ingvarch/kraken)](https://github.com/ingvarch/kraken/releases)
[![License](https://img.shields.io/github/license/ingvarch/kraken)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-dea584?logo=rust&logoColor=black)](Cargo.toml)

kraken releases a project in one command: it bumps the version, commits, tags and pushes. It is for developers who release JavaScript, Rust, Python or Go projects from their own terminal and want the version files, the lock file and the tag to agree. kraken is made by Igor Churmeev.

## Features

- Bumps patch, minor or major in `package.json`, `Cargo.toml`, `pyproject.toml` or `VERSION`, or works with git tags alone
- Changes only the version string: comments, quotes and formatting stay
- Sets the project's own entry in `package-lock.json`, `Cargo.lock` or `uv.lock`, with no network
- Refuses a dirty tree, a shallow clone, tags on origin you have not fetched and a version that is not above the latest tag
- Commits, creates an annotated tag and pushes the branch and the tag in one atomic push
- Puts HEAD and the files back if anything fails before the tag
- A full-screen panel for releases by hand, `--yes` and `--dry-run` for scripts

## Usage

```
brew install ingvarch/tap/kraken
kraken
```

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

Other ways to install, the commands and the flags are in [docs/usage.md](docs/usage.md).

## Quick start

You need Rust stable and git, see [docs/development.md](docs/development.md).

```
git clone https://github.com/ingvarch/kraken
cd kraken
cargo test
cargo run -- patch --dry-run
```

## Documentation

- [Usage](docs/usage.md): install, commands and flags, the panel, scripts and CI
- [How it works](docs/how-it-works.md): checks, version files, lock files, commit, tag, push, rollback
- [Limits](docs/limits.md): one version per repository, `X.Y.Z` only, no changelog
- [Development](docs/development.md): prerequisites, checks, project structure
- [Releasing kraken](docs/releasing.md): the release workflow and its secrets

## License

MIT, see [LICENSE](LICENSE).
