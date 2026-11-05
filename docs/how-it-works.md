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
| `pubspec.yaml` | the top-level `version`, up to the `+` |
| `composer.json` | the top-level `version`, when it is set |
| `manifest.json` (browser extension, with `manifest_version`) | the top-level `version`, and `version_name` when it equals `version` |
| `Directory.Build.props` | `Version`, `VersionPrefix` and `PackageVersion` in a `PropertyGroup`, in any letter case, as MSBuild reads them |
| `<Name>.csproj` (also `.fsproj`, `.vbproj`) | the same |
| `project.yml` (XcodeGen) | every `MARKETING_VERSION:` line |
| `<Name>.xcodeproj/project.pbxproj` | every `MARKETING_VERSION` line |
| `VERSION` | the whole file: the text without the whitespace around it |

If no file carries a version (a Go module, a `dynamic` Python version, anything else), kraken works with git tags only.

kraken skips a version file that git ignores, such as an Xcode project that a tool generates. XcodeGen is the exception: kraken still sets the version in the project it generates, see [Files it changes](#files-it-changes).

## Current and next version

kraken takes the current version from those files, or from the highest `X.Y.Z` / `vX.Y.Z` tag reachable from HEAD. Tags such as `v1.4.0-rc.1` count, in semver order: 1.4.0-rc.2 < 1.4.0-rc.10 < 1.4.0. It warns when the files and the latest tag disagree, and refuses a new version that is not higher than the latest tag. Then it asks for patch, minor or major and starts on the level the commits suggest, see [Suggested level](#suggested-level). `kraken version` prints the current version and changes nothing. It skips the branch, clean-tree and origin checks, and refuses a shallow clone only when the version comes from tags. `kraken version --next` bumps the current version at the suggested level. It runs the tag checks of a release, refuses a shallow clone even with version files, and skips the branch, clean-tree and origin checks.

## Pre-releases

`--pre alpha|beta|rc` makes the next version a pre-release. The number starts at 1 and counts up on the next run. Without `--pre` a pre-release becomes the final version.

| Current | Command | Next |
|---|---|---|
| 1.3.0 | `kraken minor --pre rc` | 1.4.0-rc.1 |
| 1.3.0 | `kraken patch --pre beta` | 1.3.1-beta.1 |
| 1.3.0 | `kraken major --pre alpha` | 2.0.0-alpha.1 |
| 1.3.0 | `kraken --pre rc` | the panel lists 1.3.1-rc.1, 1.4.0-rc.1 and 2.0.0-rc.1; with `--yes`, an error |
| 1.4.0-rc.1 | `kraken --pre rc` | 1.4.0-rc.2 |
| 1.4.0-beta.2 | `kraken --pre rc` | 1.4.0-rc.1 |
| 1.4.0-rc.2 | `kraken --pre beta` | refused: a pre-release goes from alpha to beta to rc |
| 1.4.0-rc.2 | `kraken minor` | 1.4.0 |
| 1.4.0-rc.2 | `kraken major` | 2.0.0 |
| 1.4.0-rc.2 | `kraken major --pre rc` | 2.0.0-rc.1 |
| 1.4.0-rc.2 | `kraken patch` | refused: 1.4.0 is a minor release |
| 1.4.3-rc.1 | `kraken minor` | 1.5.0 |
| 1.4.0-SNAPSHOT | `kraken minor` | 1.4.0 |

A pre-release leads to the final version `X.Y.Z`. Its own level is the lowest part of `X.Y.Z` that is not zero: patch for 1.4.3-rc.1, minor for 1.4.0-rc.1, major for 2.0.0-rc.1. That level gives `X.Y.Z`, a higher level bumps it, and a lower level is refused, so no two levels give the same version. kraken counts only pre-releases of the form `<label>.<number>`; any other, such as `SNAPSHOT`, starts again at `.1`. The result must be higher than the current version in semver order.

Pre-release tags made by hand or by another tool count too. With `v1.10.0` and `v2.0.0-rc.1` both reachable from HEAD, the latest tag is `v2.0.0-rc.1`, and `kraken patch` is refused as not higher. A 1.x maintenance release belongs on a branch where `v2.0.0-rc.1` is not reachable.

The tag uses the semver spelling in every project, `v1.4.0-rc.1`. A Python release workflow that compares the tag with `pyproject.toml` must compare parsed versions: `1.4.0rc1` is the same version as `1.4.0-rc.1`.

## Suggested level

kraken reads the commits from the latest final release tag to HEAD, or every commit when there is no release tag. Merge commits do not count; the commits they bring in do. With squash merges the pull request title is the subject, so it decides. It reads each commit by the same rules as the changelog, and the highest level wins:

| Commit | Kind | Level |
|---|---|---|
| any type with `!`, or a `BREAKING CHANGE:` or `BREAKING-CHANGE:` footer | breaking change | major, minor while the major version is 0 |
| `feat` | feature | minor |
| `fix` | fix | patch |
| `build`, `chore`, `ci`, `docs`, `perf`, `refactor`, `revert`, `style`, `test` | other commit | patch |
| any other subject | other commit | patch |

A footer under a subject that is not a conventional commit does not count. From a pre-release the suggestion is never below the pre-release's own level.

While the major version is 0, a breaking change suggests minor, and so does a feature. Semver lets anything change before 1.0.0, and Cargo and npm treat a 0.x minor bump as breaking. kraken never suggests 1.0.0. Choose `major` when you are ready to leave 0.x.

The panel's level screen starts on the suggested level and shows the counts in the `Changes` row, such as `2 features, 1 fix since v1.4.2`. The plain output prints the same counts as `Changes:` after `Current version:`. With no commits since the latest final release tag there is no suggestion, the panel starts on patch, and a release with an explicit level goes on after a warning: `! no commits since v1.4.2` in the panel, `warning: no commits since v1.4.2` on stderr with `--yes` or `--dry-run`. A first release, with no tag yet, never warns.

`kraken auto` releases at the suggested level and prints it as `Level: minor` after the `Changes:` line. In 0.x with a breaking change it prints `Level: minor (breaking changes bump minor in 0.x)`. With no commits since the latest final release tag it fails with `no commits since v1.4.2; nothing to release`, before it asks origin, so a second run in CI does not release the same code twice. With `--pre`, the check for new commits starts at the latest tag of any kind, so `kraken auto --pre rc` fails the same way right after a pre-release and the other levels warn. The count and the level still come from the latest final release tag. Everything after the level is the same as for a level given by name.

When the chosen level is lower than the suggestion, also a level from the command line, the confirmation shows `! the new commits suggest a minor release`. With `--yes` or `--dry-run`, kraken prints `warning: the new commits suggest a minor release` to stderr and carries on. A higher level shows no warning.

## Files it changes

kraken rewrites only the version string; comments, quotes and formatting stay as they were. Every file gets the same version in its own spelling. `package.json`, `Cargo.toml`, `deno.json`, `jsr.json`, `gradle.properties`, `VERSION`, `pubspec.yaml`, `composer.json`, `Directory.Build.props`, `.csproj` files and their lock files get `1.4.0-rc.1` as it is. `pyproject.toml` and `uv.lock` get the normalized PEP 440 spelling `1.4.0rc1` (`1.4.0b1` for beta and `1.4.0a1` for alpha), and kraken reads it back as the same version. A repository with an Xcode project, an XcodeGen `project.yml` or a browser extension's `manifest.json` refuses a pre-release, because `MARKETING_VERSION` and the extension's `version` take numbers only. Version files that disagree are compared as versions, so `1.4.0-rc.1` and `1.4.0rc1` agree. The panel and `--dry-run` say when a file gets another spelling. A Cargo workspace whose path dependencies have no `version` accepts a pre-release, as cargo does. It sets the project's own entry in the lock file, if there is one: `package-lock.json`, `Cargo.lock` or `uv.lock`. For `composer.lock` kraken sets `content-hash`, the hash Composer keeps of `composer.json`, and refuses a `composer.lock` whose hash does not match `composer.json`. For XcodeGen, the generated `<name>.xcodeproj/project.pbxproj`, named after `name` in `project.yml`, plays the lock file's part and gets the same version; other Xcode projects stay as they are. Nothing else in the lock file changes, and no network is needed. For Cargo, kraken reads the workspace members with `cargo metadata --no-deps` and refuses a version that a path dependency inside the workspace would not accept. If the repository root has a `CHANGELOG.md` with the kraken line, kraken adds a section to it, see [Changelog](#changelog).

## Changelog

`kraken changelog` creates `CHANGELOG.md` in the repository root with a short header and this line, followed by the past releases, see [The history](#the-history):

```
<!-- kraken adds a section for every release. Delete this line to stop it. -->
```

Once the file is committed, every release adds a section to it. kraken writes only to a `CHANGELOG.md` that has this line, so a changelog kept by hand or by another tool stays as it is. GitHub does not show the line. To have kraken write to your own changelog, add the line to it on a line of its own, not indented; to stop kraken, delete the line. `--no-changelog` leaves the file out of one release. The name must be exactly `CHANGELOG.md`, a symlink does not count, and a file with the line must be UTF-8. If git does not track a `CHANGELOG.md` with the line, kraken refuses to release until you commit it.

A pre-release adds no section and leaves `CHANGELOG.md` out of its commit; the panel and `--dry-run` say that the section waits for the final release. The final release lists every commit since the latest final release, so the `1.4.0` section holds the commits of all its release candidates. The section lists the commits since the latest final release tag, or every commit for a first release, oldest first and without merge commits. kraken reads each subject as a conventional commit, `type(scope): description`, with one of the standard types:

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

The date is the day kraken starts, in the time zone git gives commits: kraken reads it from `git var GIT_COMMITTER_IDENT`, so `GIT_COMMITTER_DATE` sets it too. kraken puts the section above the first `## ` heading other than `## [Unreleased]`, which in a Keep a Changelog file is the latest release, or at the end of the file, above the link definitions there, when there is no such heading. The rest of the file stays as it is. The panel and `--dry-run` show the section before the release.

### The history

`kraken changelog` writes a section for every past release, so the file is complete from the start. It takes the final release tags that HEAD contains: `X.Y.Z` or `vX.Y.Z`. Pre-release tags, other tags and tags on branches not merged into HEAD get no section. When `1.2.0` and `v1.2.0` both exist, kraken uses the `v` one.

kraken sorts the tags by version. The section of a tag lists the commits after the final tag just below it, up to and including the tagged commit, oldest first and without merge commits. The lowest tag lists every commit up to it. The commits of a pre-release tag go into the next final release. The groups are the ones a release uses, so `chore(release): v1.2.0` is left out. A release with no entries keeps its heading. Commits after the latest tag get no section; the next release adds them.

The date of a section is the date of its tag: the tagger date of an annotated tag, the commit date of a lightweight tag, in the time zone of each. The sections come newest first, below the header and the kraken line. When origin is on a known host, the file ends with a link for every release, see below. The command reads git and changes nothing in it. It needs no clean tree, no branch and no network.

### Compare links

Keep a Changelog ends the file with link definitions, so each version heading opens the page that compares it with the previous release. When origin is on github.com, gitlab.com or codeberg.org, a release adds the definition for its version above the latest release's one:

```
[Unreleased]: https://github.com/o/r/compare/v1.4.0...HEAD
[1.4.0]: https://github.com/o/r/compare/v1.3.0...v1.4.0
[1.0.0]: https://github.com/o/r/releases/tag/v1.0.0
```

The link compares the latest final release tag with the new tag, both as they are written, so `v1.3.0` or `1.3.0`. A first release links the tag page. An `[Unreleased]` definition that compares from the previous tag, such as `compare/v1.3.0...HEAD`, moves to the new tag. kraken keeps its label and the part after the tag, and never adds one. A pre-release adds no section, so it adds no link.

The file decides whether it gets links. A file whose link definitions at the end include a version keeps getting them. A file with no release section yet, such as the one `kraken changelog` makes in a repository without final release tags in HEAD, starts them with its first release. A file with release sections and no version links gets none. To start links in such a file, add the definitions for the past releases, or at least the latest. To stop them, delete the version definitions. `--no-changelog` adds no links.

kraken reads the URL origin pushes to, `git remote get-url --push origin`, with `insteadOf` and `pushInsteadOf` applied. It understands HTTPS, `git@host:owner/repo.git` and `ssh://` URLs, with or without `.git`. For an SSH host it runs `ssh -G`, which reads `~/.ssh/config` and connects to nothing, so `git@github-work:o/r.git` with `HostName github.com` gets GitHub links. It builds the address from the host and the path alone. A user name, password, token or port in the URL never reaches the file or the output. The links need no network. `--dry-run` shows the link lines, and a failed release puts the file back with its links as they were.

## Commit and tag

kraken commits `chore(release): v1.2.3` and creates an annotated tag. The tag keeps the style of the latest tag (`v1.2.3` or `1.2.3`); the default is `v`. The tag is always in semver, so a pre-release is `v1.4.0-rc.1`. A lock file that git does not track is updated but left out of the commit. With no version files and no changelog there is nothing to commit, so kraken only tags HEAD.

## Push

If there is an `origin` remote, kraken asks whether to push the branch and the tag. The branch goes to its upstream branch on origin, or to a branch with the same name. Both go in one atomic push.

## GitHub release

With `--github-release`, kraken makes a GitHub release for the new tag right after a successful push. It does this only when you ask. Without the flag it never runs gh.

kraken runs gh for this and adds no HTTP client of its own. gh already handles the login, the keyring, `GH_TOKEN`, token refresh and GitHub Enterprise hosts.

The release goes to the repository that origin pushes to. kraken reads the URL the same way as for the changelog links, `git remote get-url --push origin`, and the path must be exactly `OWNER/REPO`. For an SSH URL it runs `ssh -G`, so `git@github-work:o/r.git` with `HostName github.com` goes to `github.com`, and `ssh.github.com` counts as `github.com`. A user name or password in the URL never shows in the output. kraken passes `--repo` to gh itself. Left alone, gh may pick a remote named `upstream` or the one set by `gh repo set-default`, which in a fork is not the repository kraken pushed to. With `--repo`, other remotes, `GH_REPO` and `gh repo set-default` change nothing.

Before anything changes kraken refuses when there is no origin, when the push URL names no `HOST/OWNER/REPO`, when gh is not installed and when this call fails:

```
gh api --hostname github.com repos/o/r --silent
```

It succeeds when gh has a token for the host and the token can see the repository. `--dry-run` makes the call too. kraken does not use `gh auth status` or `gh auth token`.

After the push kraken runs:

```
gh release create v1.4.0 --repo github.com/o/r --verify-tag --title v1.4.0 --notes-file /home/me/app/.git/KRAKEN_RELEASE_NOTES.md
```

- The title is the tag as it is, `v1.4.0` or `1.4.0`.
- `--verify-tag` makes gh stop when the tag is missing on GitHub, so gh never makes a tag of its own.
- The notes are the changelog section without its `## [X.Y.Z] - date` heading. Without a `CHANGELOG.md` that has the kraken line, or with `--no-changelog`, kraken builds the same section from the same commits and leaves the file alone. A section without entries gives a release without notes. GitHub takes at most 125000 characters, and kraken refuses longer notes before anything changes.
- A pre-release goes out with `--prerelease`. Its notes list the commits since the latest tag of any kind, because a pre-release adds no changelog section.
- A release below the highest final release tag, such as 1.9.1 on a maintenance branch when `v2.0.0` exists, goes out with `--latest=false`, so GitHub keeps the newer release as Latest.
- kraken writes the notes to `KRAKEN_RELEASE_NOTES.md` in the git directory right before the call and deletes the file when gh succeeds. The path is absolute, and each linked worktree gets its own file in its own git directory.

With the push turned off in the panel there is no GitHub release, and kraken says so. The panel shows where the release goes and its notes, and `--dry-run` shows the same.

When gh fails after the push, the commit and the tag are on origin, and kraken rolls nothing back. It exits with 1, keeps the notes file and prints the command to try again. Running that command twice is safe, because GitHub refuses a second published release for the same tag. After a timeout the release may exist already, and the retry then fails with GitHub's error that says so.

## Rollback

If anything fails before the tag exists, kraken moves HEAD back and puts the files back as they were, `CHANGELOG.md` included. The GitHub release comes after the push, so a failed one rolls nothing back. For a release that went through, use [`kraken undo`](#undo).

## Undo

`kraken undo` removes one release: the highest release tag reachable from HEAD. The tag must point at HEAD, so a release with commits after it is refused.

The tag must be kraken's: annotated, with the tag name as the message. A lightweight tag, or a tag with another message such as the one `npm version` writes, is refused.

kraken tells a release commit from a tag-only release by the subject of HEAD:

- The subject is `chore(release): <tag>`. This is a release commit. It must have one parent and may change only version files, their lock files and `CHANGELOG.md`.
- Any other subject, and there are version files. kraken would have made a commit, so it did not make this release, and undo refuses.
- Any other subject, and no version files. The release only tagged HEAD, and undo deletes the tag.

Undo removes only a release that is not pushed. No remote-tracking branch may contain the release commit. When there is an `origin` remote, kraken also asks origin with `git ls-remote`. The tag is pushed when origin has the same tag object. The commit is pushed when a branch on origin points at it or at a commit that contains it. A tag on origin with the same name but another object does not count: someone else released that version first. If origin does not answer, undo refuses. A release only warns in this case, but undo deletes things and has to know. Other remotes are not asked.

After you confirm, kraken checks again that nothing changed, runs `git reset --keep` to the parent of the release commit, and deletes the tag. `--keep` stops instead of throwing a change away, and untracked files stay. A lock file that git does not track, such as an ignored `Cargo.lock`, gets the version of the restored version files, because git cannot put it back. Last, kraken prints the current version.
