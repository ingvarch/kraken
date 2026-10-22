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

kraken takes the current version from those files, or from the highest `X.Y.Z` / `vX.Y.Z` tag reachable from HEAD. It warns when the files and the latest tag disagree, and refuses a new version that is not higher than the latest tag. Then it asks for patch, minor or major.

## Files it changes

kraken rewrites only the version string; comments, quotes and formatting stay as they were. It sets the project's own entry in the lock file, if there is one: `package-lock.json`, `Cargo.lock` or `uv.lock`. For XcodeGen, the generated `<name>.xcodeproj/project.pbxproj`, named after `name` in `project.yml`, plays the lock file's part and gets the same version; other Xcode projects stay as they are. Nothing else in the lock file changes, and no network is needed. For Cargo, kraken reads the workspace members with `cargo metadata --no-deps` and refuses a version that a path dependency inside the workspace would not accept.

## Commit and tag

kraken commits `chore(release): v1.2.3` and creates an annotated tag. The tag keeps the style of the latest tag (`v1.2.3` or `1.2.3`); the default is `v`. A lock file that git does not track is updated but left out of the commit. With no version files there is nothing to commit, so kraken only tags HEAD.

## Push

If there is an `origin` remote, kraken asks whether to push the branch and the tag. The branch goes to its upstream branch on origin, or to a branch with the same name. Both go in one atomic push.

## Rollback

If anything fails before the tag exists, kraken moves HEAD back and puts the files back as they were.
