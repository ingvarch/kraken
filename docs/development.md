# Development

Local setup, checks and project layout. To publish a new version of kraken, see [releasing.md](releasing.md).

## Prerequisites

- Rust stable with edition 2024 (1.85 or newer).
- `git` on `PATH`: the tests create temporary git repositories.
- [cargo-about](https://github.com/EmbarkStudios/cargo-about) for the license check.
- For a release snapshot: GoReleaser (the version is in `.tool-versions`), Zig 0.15.1 and `cargo-zigbuild`.

## Checks

The `ci` workflow runs these on every push to `main` and on every pull request:

```
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo about generate --fail --output-file /dev/null about.hbs
cargo test --locked
```

The license check fails on a crate whose license `about.toml` does not accept. The tests run on Linux and macOS.

CI also builds a release snapshot, to find a broken release before a tag does:

```
goreleaser release --snapshot --clean
```

## Project structure

- `src/main.rs`: the command line and the plain output; starts the panel.
- `src/ui.rs`: the ratatui panel.
- `src/release.rs`: checks the repository, plans the release, writes the files, commits, tags, pushes and rolls back.
- `src/manifest.rs` and `src/manifest/`: version files and lock files, one module per format.
- `src/version.rs`: versions, levels and release tags.
- `src/git.rs`: the git commands kraken runs.
- `src/cmd.rs`: runs a program with a time limit and stops it with everything it started.
- `tests/cli.rs`: runs the binary against temporary git repositories.
