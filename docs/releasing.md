# Releasing kraken

How a new version of kraken gets built, signed and published. Local setup and checks are in [development.md](development.md).

## Release

Release kraken with kraken: `kraken` in this repository bumps `Cargo.toml`, tags and pushes. kraken is released without `--github-release`, because GoReleaser makes the GitHub release in the step "Publishes the GitHub release". The tag starts the `release` workflow, which:

1. Stops if the tag and the version in `Cargo.toml` differ.
2. Builds the archives with GoReleaser for Linux and macOS, x86_64 and aarch64, and puts `LICENSE`, `README.md`, `THIRD_PARTY_NOTICES`, the guides from `docs/` and the shell completions in `completions/` in each. GoReleaser makes the completion scripts with `kraken completions` from a host build of the commit it releases. The `release snapshot` job in CI checks that every archive has each guide the README links to and the three completion scripts, and that the cask lists them.
3. Builds `.deb` and `.rpm` packages for Linux x86_64 and aarch64 with nfpm. The package is named `kraken-release`, files are `kraken-release_X.Y.Z_amd64.deb` and `kraken-release-X.Y.Z-1.x86_64.rpm`. Each has the binary in `/usr/bin`, the completions, and the README, license files and guides in `/usr/share/doc/kraken-release/`. The .deb depends on `git` and conflicts with Debian's `kraken`, the .rpm depends on `git-core`. A pre-release tag such as `v0.5.0-rc.1` becomes `0.5.0~rc.1`, which sorts before `0.5.0`. The file names have `.` in place of `~`, because GitHub changes `~` to `.` in asset names. The `release snapshot` job in CI installs the amd64 .deb on the runner and the x86_64 .rpm in a Fedora container, and runs `kraken --version` after each.
4. Signs and notarizes the macOS binaries.
5. Publishes the GitHub release with the archives, the packages and the checksums file.
6. Updates the cask in [ingvarch/homebrew-tap](https://github.com/ingvarch/homebrew-tap). The cask installs the bash, zsh and fish completions.

## crates.io

The crate is `kraken-release` because the name `kraken` on crates.io belongs to another project. The binary and the command are still `kraken`. `cargo binstall kraken-release` reads `[package.metadata.binstall]` in `Cargo.toml` and downloads the matching archive from the GitHub release. The metadata turns off the third-party builds binstall would otherwise fall back to, so a target without an archive builds from source.

Trusted publishing works only for a crate that already exists, so publish the first version by hand:

1. Release the version with kraken as usual.
2. On a clean checkout of its tag, create a crates.io token with the scope `publish-new` for `kraken-release` and a short expiry. The account needs a verified email address.
3. Run `cargo publish --locked`, then delete the token.
4. On crates.io, open kraken-release, Settings, Trusted Publishing. Add GitHub with owner `ingvarch`, repository `kraken` and workflow `release.yml`. You can also turn on the setting that allows trusted publishing only.
5. Add a `crate` job to `release.yml`:

   ```yaml
     crate:
       name: crates.io
       needs: release
       runs-on: ubuntu-26.04
       permissions:
         contents: read
         id-token: write
       steps:
         - uses: actions/checkout@v7

         - uses: dtolnay/rust-toolchain@stable

         - uses: rust-lang/crates-io-auth-action@v1
           id: auth

         - name: publish
           run: cargo publish --locked
           env:
             CARGO_REGISTRY_TOKEN: ${{ steps.auth.outputs.token }}
   ```

   The action trades GitHub's OIDC token for a crates.io token that lives 30 minutes, so the repository keeps no crates.io secret. A crates.io version cannot be deleted, which is why the job runs after the GitHub release. If it fails, rerun it alone from the Actions page.

Until step 5 is done, the release workflow does not publish the crate. From the next tag after that, it publishes each version.

`kraken undo` cannot take back a published version. To hide a bad one from new installs, run `cargo yank --version X.Y.Z kraken-release`. A pre-release such as `0.5.0-rc.1` is published like any other version, and `cargo install` picks it only with `--version`.

## Secrets

The `release` workflow reads these repository secrets:

| Secret | What it is |
|---|---|
| `HOMEBREW_TAP_GITHUB_TOKEN` | a token that can push to `ingvarch/homebrew-tap` |
| `MACOS_SIGN_P12` | the Developer ID Application certificate, a base64 `.p12` |
| `MACOS_SIGN_PASSWORD` | the certificate's password |
| `MACOS_NOTARY_ISSUER_ID` | the App Store Connect API issuer ID |
| `MACOS_NOTARY_KEY_ID` | the App Store Connect API key ID |
| `MACOS_NOTARY_KEY` | the App Store Connect API key, a base64 `.p8` |
