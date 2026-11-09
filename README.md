# kraken

[![CI](https://github.com/ingvarch/kraken/actions/workflows/ci.yml/badge.svg)](https://github.com/ingvarch/kraken/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ingvarch/kraken)](https://github.com/ingvarch/kraken/releases)
[![License](https://img.shields.io/github/license/ingvarch/kraken)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-dea584?logo=rust&logoColor=black)](Cargo.toml)

kraken releases a project in one command: it bumps the version, commits, tags and pushes. It is for developers who release JavaScript, Deno, Rust, Python, PHP, Dart, Flutter, Gradle, .NET, Xcode, XcodeGen, browser extension or Go projects from their own terminal and want the version files, the lock file and the tag to agree. kraken is made by Igor Churmeev.

## Features

- Bumps the version in `package.json`, `Cargo.toml`, `pyproject.toml`, `deno.json`, `jsr.json`, `gradle.properties`, `pubspec.yaml`, `composer.json`, a browser extension's `manifest.json`, MSBuild, Xcode and XcodeGen projects or `VERSION`, or works with git tags alone
- Changes only the version string and updates the lock file (`package-lock.json`, `Cargo.lock`, `uv.lock`, `composer.lock`) with no network
- Suggests patch, minor or major from the conventional commits since the latest final release; `kraken auto --yes` takes the suggestion in scripts
- Releases pre-releases such as `1.4.0-rc.1` with `--pre alpha|beta|rc`
- `kraken changelog` creates `CHANGELOG.md` with the past releases, and every release then adds its section, with compare links on GitHub, GitLab and Codeberg; `--github-release` makes a GitHub release with that section as notes
- Checks the repository and origin's tags first, commits, tags and pushes in one atomic push, puts HEAD and the files back if anything fails before the tag, and `kraken undo` removes a release that is not pushed
- A full-screen panel for releases by hand, `--yes` and `--dry-run` for scripts, and completions for bash, zsh and fish

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
│  Changes   2 features, 1 fix since v1.4.2                │
│                                                          │
│  Next version                                            │
│    patch   1.4.3   bug fixes                             │
│  ❯ minor   1.5.0   new features                          │
│    major   2.0.0   breaking changes                      │
│                                                          │
├──────────────────────────────────────────────────────────┤
│  ↑↓ choose   enter continue   esc quit                   │
╰──────────────────────────────────────────────────────────╯
```

Other ways to install (cargo, .deb and .rpm packages, archives), the commands and the flags are in [docs/usage.md](docs/usage.md).

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
- [How it works](docs/how-it-works.md): checks, version files, lock files, changelog, commit, tag, push, rollback, undo
- [Limits](docs/limits.md): one version per repository, pre-release labels, files that take numbers only, the GitHub release
- [Development](docs/development.md): prerequisites, checks, project structure
- [Releasing kraken](docs/releasing.md): the release workflow and its secrets

## License

MIT, see [LICENSE](LICENSE).

## Contributing

Bug reports and pull requests are welcome, see [CONTRIBUTING.md](https://github.com/ingvarch/kraken/blob/main/CONTRIBUTING.md).
