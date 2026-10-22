# Usage

How to install kraken and run it, by hand or from a script. What it checks and changes is in [how-it-works.md](how-it-works.md).

## Install

```
brew install ingvarch/tap/kraken
```

Or download an archive for your platform from the [releases](https://github.com/ingvarch/kraken/releases), or build from source with `cargo install --path .`. kraken works on macOS and Linux.

## Commands

```
kraken [patch|minor|major] [--yes] [--dry-run]
kraken version
```

- `kraken` opens a full-screen panel: pick the level with the arrow keys (or `j`/`k`), see what the release will change, turn the push on or off with space, and release with enter. `esc` goes back, `q` quits. On enter the panel closes and kraken writes, commits, tags and pushes in the terminal, so git can ask for a passphrase or credentials there.
- `kraken minor` opens the panel on the confirmation of a minor release.
- `kraken minor --yes` opens no panel, asks nothing and pushes if `origin` exists. Use this in scripts.
- `kraken minor --dry-run` shows what would change and changes nothing.
- `kraken version` prints the version kraken would release from, for example `1.4.2`, and changes nothing. It needs no clean tree or branch and does not touch the network. In a shallow clone it needs version files, and it cannot compare them with tags the clone did not fetch; with tags alone run `git fetch --unshallow --tags` first. A warning about a differing tag goes to stderr, so `$(kraken version)` holds only the version.

## Without a terminal

In CI or a pipe there is no panel, so a release needs the level and `--yes`.
