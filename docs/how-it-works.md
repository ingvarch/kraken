# How it works

What kraken checks, which files it changes, and how it commits, tags and pushes. What it does not support is in [limits.md](limits.md).

## Checks

kraken checks that you are in a git repository, on a branch, with no uncommitted changes to tracked files. A shallow clone is refused because it may miss tags. If there is an `origin` remote, kraken asks it for its tags and refuses to go on while origin has release tags you have not fetched. If origin cannot be reached, kraken warns and goes on.

## Version files

kraken looks for version files in the repository root:

| File | Version field |
|---|---|
| `package.json` | `version` |
| `Cargo.toml` | `[package].version` or `[workspace.package].version` |
| `pyproject.toml` | `[project].version` or `[tool.poetry].version` |
| `deno.json` or `deno.jsonc` | `version` |
| `jsr.json` or `jsr.jsonc` | `version` |
| `gradle.properties` | the `version` line |
| `project.yml` (XcodeGen) | every `MARKETING_VERSION:` line |
| `<Name>.xcodeproj/project.pbxproj` | every `MARKETING_VERSION` line |
| `VERSION` | the whole file: the text without the whitespace around it |

If no file carries a version (a Go module, a `dynamic` Python version, anything else), kraken works with git tags only.

kraken skips a version file that git ignores, such as an Xcode project that a tool generates. XcodeGen is the exception: kraken still sets the version in the project it generates, see [Files it changes](#files-it-changes).

## Current and next version

kraken takes the current version from those files, or from the highest `X.Y.Z` / `vX.Y.Z` tag reachable from HEAD. It warns when the files and the latest tag disagree, and refuses a new version that is not higher than the latest tag. Then it asks for patch, minor or major and starts on the level the commits suggest, see [Suggested level](#suggested-level). `kraken version` prints the current version and changes nothing. It skips the branch, clean-tree and origin checks, and refuses a shallow clone only when the version comes from tags. `kraken version --next` bumps the current version at the suggested level. It runs the tag checks of a release, refuses a shallow clone even with version files, and skips the branch, clean-tree and origin checks.

## Suggested level

kraken reads the commits from the latest release tag to HEAD, or every commit when there is no release tag. Merge commits do not count; the commits they bring in do. With squash merges the pull request title is the subject, so it decides. It reads each commit by the same rules as the changelog, and the highest level wins:

| Commit | Kind | Level |
|---|---|---|
| any type with `!`, or a `BREAKING CHANGE:` or `BREAKING-CHANGE:` footer | breaking change | major, minor while the major version is 0 |
| `feat` | feature | minor |
| `fix` | fix | patch |
| `build`, `chore`, `ci`, `docs`, `perf`, `refactor`, `revert`, `style`, `test` | other commit | patch |
| any other subject | other commit | patch |

A footer under a subject that is not a conventional commit does not count.

While the major version is 0, a breaking change suggests minor, and so does a feature. Semver lets anything change before 1.0.0, and Cargo and npm treat a 0.x minor bump as breaking. kraken never suggests 1.0.0. Choose `major` when you are ready to leave 0.x.

The panel's level screen starts on the suggested level and shows the counts in the `Changes` row, such as `2 features, 1 fix since v1.4.2`. The plain output prints the same counts as `Changes:` after `Current version:`. With no commits since the latest tag there is no suggestion, the panel starts on patch, and a release with an explicit level works as before.

`kraken auto` releases at the suggested level and prints it as `Level: minor` after the `Changes:` line. In 0.x with a breaking change it prints `Level: minor (breaking changes bump minor in 0.x)`. With no commits since the latest tag it fails with `no commits since v1.4.2; nothing to release`, before it asks origin, so a second run in CI does not release the same code twice. Everything after the level is the same as for a level given by name.

When the chosen level is lower than the suggestion, also a level from the command line, the confirmation shows `! the new commits suggest a minor release`. With `--yes` or `--dry-run`, kraken prints `warning: the new commits suggest a minor release` to stderr and carries on. A higher level shows no warning.

## Files it changes

kraken rewrites only the version string; comments, quotes and formatting stay as they were. It sets the project's own entry in the lock file, if there is one: `package-lock.json`, `Cargo.lock` or `uv.lock`. For XcodeGen, the generated `<name>.xcodeproj/project.pbxproj`, named after `name` in `project.yml`, plays the lock file's part and gets the same version; other Xcode projects stay as they are. Nothing else in the lock file changes, and no network is needed. For Cargo, kraken reads the workspace members with `cargo metadata --no-deps` and refuses a version that a path dependency inside the workspace would not accept. If the repository root has a `CHANGELOG.md` with the kraken line, kraken adds a section to it, see [Changelog](#changelog).

## Changelog

`kraken changelog` creates `CHANGELOG.md` in the repository root with a short header and this line:

```
<!-- kraken adds a section for every release. Delete this line to stop it. -->
```

Once the file is committed, every release adds a section to it. kraken writes only to a `CHANGELOG.md` that has this line, so a changelog kept by hand or by another tool stays as it is. GitHub does not show the line. To have kraken write to your own changelog, add the line to it on a line of its own, not indented; to stop kraken, delete the line. `--no-changelog` leaves the file out of one release. The name must be exactly `CHANGELOG.md`, a symlink does not count, and a file with the line must be UTF-8. If git does not track a `CHANGELOG.md` with the line, kraken refuses to release until you commit it.

The section lists the commits since the latest release tag, or every commit for a first release, oldest first and without merge commits. kraken reads each subject as a conventional commit, `type(scope): description`, with one of the standard types:

| Commit | Group |
|---|---|
| any type with `!`, such as `feat!:`, or with a `BREAKING CHANGE:` footer | Breaking |
| `feat` | Added |
| `fix` | Fixed |
| `build`, `chore`, `ci`, `docs`, `perf`, `refactor`, `revert`, `style`, `test` | left out |
| any other subject, such as `Update README` or `parser: handle empty input` | Changed |

The groups come in the order Breaking, Added, Changed, Fixed. An entry is the description, with the scope in bold before it when the commit has one; a Changed entry is the whole subject:

```
## [1.5.0] - 2026-11-04

### Added

- **ui**: add the release panel
- read the version from gradle.properties
```

The date is the day kraken starts, in the time zone git gives commits: kraken reads it from `git var GIT_COMMITTER_IDENT`, so `GIT_COMMITTER_DATE` sets it too. kraken puts the section above the first `## ` heading other than `## [Unreleased]`, which in a Keep a Changelog file is the latest release, or at the end of the file when there is no such heading. The rest of the file stays as it is. The panel and `--dry-run` show the section before the release.

## Commit and tag

kraken commits `chore(release): v1.2.3` and creates an annotated tag. The tag keeps the style of the latest tag (`v1.2.3` or `1.2.3`); the default is `v`. A lock file that git does not track is updated but left out of the commit. With no version files and no changelog there is nothing to commit, so kraken only tags HEAD.

## Push

If there is an `origin` remote, kraken asks whether to push the branch and the tag. The branch goes to its upstream branch on origin, or to a branch with the same name. Both go in one atomic push.

## Rollback

If anything fails before the tag exists, kraken moves HEAD back and puts the files back as they were, `CHANGELOG.md` included.
