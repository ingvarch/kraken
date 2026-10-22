# Limits

What kraken does not support. What it does support is in [how-it-works.md](how-it-works.md).

- One version for the whole repository. A Cargo workspace needs `[workspace.package].version`; per-crate versions are not supported.
- kraken does not change dependency requirements. If `app` depends on `engine = { path = "../engine", version = "0.1.0" }`, a bump to 0.2.0 is refused until you update the requirement.
- Only `X.Y.Z` versions. Pre-release (`1.0.0-rc.1`) and build metadata are rejected; pre-release tags are ignored.
- No changelog and no GitHub releases.
- A version set in `build.gradle` or `build.gradle.kts` is code, and kraken does not read it. Put the version in `gradle.properties`.
