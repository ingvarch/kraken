# Limits

What kraken does not support. What it does support is in [how-it-works.md](how-it-works.md).

- One version for the whole repository. A Cargo workspace needs `[workspace.package].version`; per-crate versions are not supported.
- kraken does not change dependency requirements. If `app` depends on `engine = { path = "../engine", version = "0.1.0" }`, a bump to 0.2.0 is refused until you update the requirement.
- Build metadata (`1.0.0+abc`) is refused, except the build number after `+` in `pubspec.yaml`.
- kraken makes only `alpha`, `beta` and `rc` pre-releases, in the form `1.4.0-rc.1`. It reads other pre-releases, such as `1.4.0-SNAPSHOT`, as the current version, and a release from them gives the final version or starts at `.1`.
- kraken reads `pyproject.toml` and `uv.lock` only as `X.Y.Z` or in the normalized PEP 440 spelling `1.4.0rc1`, `1.4.0b1` or `1.4.0a1`. Other spellings, such as `1.4.0-rc.1` or `1.4.0.dev1`, are refused.
- An Xcode project, an XcodeGen `project.yml` and a browser extension's `manifest.json` take numbers only, so kraken refuses a pre-release there. `composer.json` takes the labels alpha, beta, RC, patch and dev. A file with `VersionPrefix` refuses a pre-release, because the label belongs in `VersionSuffix`.
- A Flutter app's iOS build turns `1.4.0-rc.1` into `1.4.0.1`. pub and Android take the version as it is, and kraken does not warn.
- A path dependency with an explicit `version = "*"` cannot be told from one without `version`. kraken accepts both for a pre-release, but cargo refuses the first.
- No GitHub releases.
- `kraken undo` removes only the latest release, only at HEAD and only when it is not pushed. It asks only `origin` over the network and does not delete tags on origin.
- The suggested level comes from the commit subjects and `BREAKING CHANGE:` footers. kraken does not compare the code or the public API.
- kraken never suggests 1.0.0: leave 0.x with `major` when you are ready.
- Changelog entries are the commit subjects as they are; kraken does not let you edit them before the release.
- Only `CHANGELOG.md` in the repository root, as a regular file. kraken does not read `changelog.md`, `CHANGES.md`, `HISTORY.md`, a changelog in a subdirectory or a `CHANGELOG.md` that is a symlink.
- kraken takes the first `## ` heading other than `## [Unreleased]` for the latest release. In a changelog that uses `##` for something else above the releases, such as `## Notes`, the new section goes above that heading.
- kraken does not move what you wrote under `## [Unreleased]` into the new section. They stay there after the release; move them into the release section yourself.
- A version set in `build.gradle` or `build.gradle.kts` is code, and kraken does not read it. Put the version in `gradle.properties`.
- In an Xcode project kraken sets `MARKETING_VERSION` and leaves `CURRENT_PROJECT_VERSION` (the build number) as it is. kraken refuses a repository with more than one `.xcodeproj` that sets `MARKETING_VERSION`; a project that git ignores does not count. Every `MARKETING_VERSION` in `project.pbxproj` (or `project.yml`) must be the same `X.Y.Z` version, test targets included. Xcode starts new targets at `1.0`, so set them to the app's version once before the first release.
- kraken reads `composer.json` only when it sets `version`. Composer advises leaving it out; then kraken works with tags. A `v` before the version is refused.
- In `pubspec.yaml` kraken sets the version before `+` and leaves the build number after it as it is, as it does with `CURRENT_PROJECT_VERSION` in Xcode.
- In a browser extension's `manifest.json` the version must have three numbers; set `1.0` to `1.0.0` once. A `version_name` that differs from `version` is refused. kraken reads `manifest.json` only in the root, and only when it has `manifest_version`; an extension built from `src/manifest.json` or `public/manifest.json` is not read.
- In MSBuild files kraken sets `Version`, `VersionPrefix` and `PackageVersion`, and leaves `VersionSuffix`, `AssemblyVersion`, `FileVersion` and `InformationalVersion` as they are. It reads `Directory.Build.props` and a `.csproj`, `.fsproj` or `.vbproj` in the root only; a version in a project in a subdirectory or in `Directory.Build.targets` is not read. kraken refuses a repository with more than one project file that sets a version. A version set from another property, such as `$(VersionPrefix)`, is refused.
