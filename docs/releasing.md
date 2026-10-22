# Releasing kraken

How a new version of kraken gets built, signed and published. Local setup and checks are in [development.md](development.md).

## Release

Release kraken with kraken: `kraken` in this repository bumps `Cargo.toml`, tags and pushes. The tag starts the `release` workflow, which:

1. Stops if the tag and the version in `Cargo.toml` differ.
2. Builds the archives with GoReleaser for Linux and macOS, x86_64 and aarch64, and puts `LICENSE`, `README.md` and `THIRD_PARTY_NOTICES` in each.
3. Signs and notarizes the macOS binaries.
4. Publishes the GitHub release.
5. Updates the cask in [ingvarch/homebrew-tap](https://github.com/ingvarch/homebrew-tap).

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
