# AGENTS.md

kraken is a Rust command-line tool that releases a project: it bumps the version files, commits, tags and pushes. The code is in `src/` (version file formats in `src/manifest/`, one module per format family, dispatched from `src/manifest.rs`), the CLI tests in `tests/cli.rs`, and the release pipeline in `.goreleaser.yaml` and `.github/workflows/`. The crate and the Linux packages are named `kraken-release`; the binary and the command are `kraken`.

- [CONTRIBUTING.md](CONTRIBUTING.md): read before writing code or committing. It has the steps of a change: branch name, test first, checks, which docs to update, commit types and scopes.
- [docs/development.md](docs/development.md): read before changing code or running checks. It has the prerequisites, the commands CI runs and the project structure.
- [docs/how-it-works.md](docs/how-it-works.md): read before changing what kraken checks, which files it writes, the changelog, commit, tag, push, the GitHub release, rollback or undo.
- [docs/limits.md](docs/limits.md): read before adding a version file format or changing what kraken refuses. It lists what kraken does not support.
- [docs/usage.md](docs/usage.md): read before changing a command, a flag, the panel keys or the install instructions.
- [docs/releasing.md](docs/releasing.md): read before releasing kraken or changing `.goreleaser.yaml`, `.github/workflows/release.yml`, `.github/release.yml` or the crate metadata. It has the release steps, the release notes, crates.io and the secrets.

## Rules

- A PR carries a type label, the area labels it touches (`ui`, `git`, `manifests`, `release`) and the milestone of its issue.
- Docs and messages use plain words and short sentences, no emoji.
