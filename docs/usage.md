# Usage

How to install kraken and run it, by hand or from a script. What it checks and changes is in [how-it-works.md](how-it-works.md).

## Install

```
brew install ingvarch/tap/kraken
```

Or download an archive for your platform from the [releases](https://github.com/ingvarch/kraken/releases), or build from source with `cargo install --path .`. kraken works on macOS and Linux.

## Commands

```
kraken [patch|minor|major|auto] [--pre alpha|beta|rc] [--yes] [--dry-run] [--no-changelog]
kraken version [--next [--pre alpha|beta|rc]]
kraken changelog
kraken undo [--yes] [--dry-run]
```

- `kraken` opens a full-screen panel. It starts on the level the commits since the latest final release tag suggest and counts them in the `Changes` row. Pick the level with the arrow keys (or `j`/`k`), see what the release will change and what it adds to the changelog, turn the push on or off with space, and release with enter. `esc` goes back, `q` quits. On enter the panel closes and kraken writes, commits, tags and pushes in the terminal, so git can ask for a passphrase or credentials there.
- `kraken minor` opens the panel on the confirmation of a minor release. If the commits suggest a higher level, the confirmation warns about it.
- `kraken auto` opens the panel on the confirmation of the level the commits suggest. `esc` goes to the level screen.
- `kraken auto --yes` takes the level from the commits, opens no panel and pushes if `origin` exists. Use this in scripts. With no commits since the latest final release tag it fails with `no commits since v1.4.2; nothing to release` and changes nothing.
- `kraken patch`, `minor` and `major` also release when there are no commits since the latest final release tag, for example to go from 0.9.5 to 1.0.0. kraken warns and goes on: the panel shows `! no commits since v0.9.5`, and with `--yes` or `--dry-run` stderr gets `warning: no commits since v0.9.5`. A first release never warns.
- `--pre alpha|beta|rc` makes a pre-release. `kraken minor --pre rc` releases 1.4.0-rc.1 from 1.3.0. `kraken --pre rc` goes on with the current pre-release: 1.4.0-rc.1 becomes 1.4.0-rc.2. `kraken minor` from 1.4.0-rc.2 releases the final 1.4.0. A pre-release adds no section to `CHANGELOG.md`. With `--pre` the panel lists pre-releases; without it the panel lists final releases. `--yes` and `--dry-run` need a level or `--pre`. See [Pre-releases](how-it-works.md#pre-releases).
- `kraken minor --yes` opens no panel, asks nothing and pushes if `origin` exists. Use this in scripts.
- `kraken minor --dry-run` shows what would change, with the whole changelog section, and changes nothing.
- `kraken minor --no-changelog` releases without adding a section to `CHANGELOG.md`. It works with the panel, `--yes` and `--dry-run`.
- `kraken version` prints the version kraken would release from, for example `1.4.2`, and changes nothing. A pre-release prints in the semver spelling, such as `1.4.0-rc.1`, in a Python project too. It needs no clean tree or branch and does not touch the network. In a shallow clone it needs version files, and it cannot compare them with tags the clone did not fetch; with tags alone run `git fetch --unshallow --tags` first. A warning about a differing tag goes to stderr, so `$(kraken version)` holds only the version.
- `kraken version --next` prints the version the next release gets, such as `1.5.0`, with the level taken from the commits, and changes nothing. With `--pre rc` it prints the pre-release that `kraken auto --pre rc` would make, such as `1.5.0-rc.1`. The changes and the level go to stderr, so `$(kraken version --next)` holds only the version. It fails with nothing to release, an existing tag or a shallow clone; in GitHub Actions use `fetch-depth: 0`.
- `kraken changelog` creates `CHANGELOG.md` in the repository root and changes nothing else. Commit the file, and every release adds a section to it, see [how-it-works.md](how-it-works.md#changelog). An existing file stays as it is: if it has the kraken line, `kraken changelog` says so; if not, it fails and prints the line to add.
- `kraken undo` removes the latest release while it is not pushed. It deletes the tag and drops the release commit, so the version files, the lock file and `CHANGELOG.md` go back to what they were. A release that only tagged HEAD loses just the tag. It lists what it will do and asks `[y/N]`; the default is no. `--yes` skips the question, and `--dry-run` only lists. Without a terminal it needs `--yes`. It checks origin first and refuses a release that is already there. See [Undo](how-it-works.md#undo).

## Without a terminal

In CI or a pipe there is no panel, so a release needs a level or `auto`, and `--yes`. In GitHub Actions, check out with `fetch-depth: 0`: kraken refuses a shallow clone.
