# AGENTS.md

kraken is a Rust command-line tool that releases a project: it bumps the version files, commits, tags and pushes. The code is in `src/` (one module per version file format in `src/manifest/`), the CLI tests in `tests/cli.rs`, and the release pipeline in `.goreleaser.yaml` and `.github/workflows/`. The crate and the Linux packages are named `kraken-release`; the binary and the command are `kraken`.

- [docs/development.md](docs/development.md): read before changing code or running checks. It has the prerequisites, the commands CI runs and the project structure.
- [docs/how-it-works.md](docs/how-it-works.md): read before changing what kraken checks, which files it writes, the changelog, commit, tag, push, the GitHub release, rollback or undo.
- [docs/limits.md](docs/limits.md): read before adding a version file format or changing what kraken refuses. It lists what kraken does not support.
- [docs/usage.md](docs/usage.md): read before changing a command, a flag, the panel keys or the install instructions.
- [docs/releasing.md](docs/releasing.md): read before releasing kraken or changing `.goreleaser.yaml`, `release.yml` or the crate metadata. It has the release steps, crates.io and the secrets.

## Rules

- A change in behavior updates the docs in the same PR: `docs/usage.md` for commands and flags, `docs/how-it-works.md` for behavior, `docs/limits.md` for what is not supported, and the README feature list when a user would notice. PRs #47, #50 and #52 show the pattern.
- Behavior changes start with a failing test: unit tests in the module, end-to-end tests in `tests/cli.rs`. The tests need no network and no gh login.
- Commit subjects and PR titles are conventional commits, `type(scope): description`. kraken releases itself and suggests the level from these subjects, so a `feat` makes the next release minor.
- A PR carries a type label, the area labels it touches (`ui`, `git`, `manifests`, `release`) and the milestone of its issue.
- Docs and messages use plain words and short sentences, no emoji.
