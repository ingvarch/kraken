# Limits

What kraken does not support. What it does support is in [how-it-works.md](how-it-works.md).

- One version for the whole repository. A Cargo workspace needs `[workspace.package].version`; per-crate versions are not supported.
- kraken does not change dependency requirements. If `app` depends on `engine = { path = "../engine", version = "0.1.0" }`, a bump to 0.2.0 is refused until you update the requirement.
- Only `X.Y.Z` versions. Pre-release (`1.0.0-rc.1`) and build metadata are rejected; pre-release tags are ignored.
- No changelog and no GitHub releases.
- A version set in `build.gradle` or `build.gradle.kts` is code, and kraken does not read it. Put the version in `gradle.properties`.
- In an Xcode project kraken sets `MARKETING_VERSION` and leaves `CURRENT_PROJECT_VERSION` (the build number) as it is. kraken refuses a repository with more than one `.xcodeproj` that sets `MARKETING_VERSION`; a project that git ignores does not count. Every `MARKETING_VERSION` in `project.pbxproj` (or `project.yml`) must be the same `X.Y.Z` version, test targets included. Xcode starts new targets at `1.0`, so set them to the app's version once before the first release.
