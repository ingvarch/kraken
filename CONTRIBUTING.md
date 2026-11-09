# Contributing to kraken

Thanks for helping. This file explains how to report problems and how to send a change.

## Report a bug or ask for a feature

Open an issue with one of the [issue templates](https://github.com/ingvarch/kraken/issues/new/choose).
For a bug, include the command you ran, kraken's output, the version files in the repository root and the output of `kraken --version`.

Do not report security problems in public issues. See [SECURITY.md](SECURITY.md).

## Set up

Local setup and the checks are described in [docs/development.md](https://github.com/ingvarch/kraken/blob/main/docs/development.md).

## Make a change

1. Create a branch from `main` named `<type>/<short-name>`, for example `fix/symlinked-version-file`.
2. Write a failing test that describes the behavior you want, then the code that makes it pass. Unit tests go in the module; end-to-end tests go in `tests/cli.rs`.
3. Run the [checks](https://github.com/ingvarch/kraken/blob/main/docs/development.md#checks). The output must have no warnings and no failures.
4. If users see the change, update `docs/usage.md` (commands and flags), `docs/how-it-works.md` (behavior), `docs/limits.md` (what is not supported) and the README feature list.
5. Commit with a [conventional commit](https://www.conventionalcommits.org/) message:
   `type(scope): description`, imperative, lower case. kraken releases itself and suggests the level from these messages, so a `feat` makes the next release minor.
   - Types: `feat`, `fix`, `refactor`, `docs`, `chore`.
   - Scopes: `manifests`, `changelog`, `cli`, `ui`, `git`, `release`, `ci`.
6. Open a pull request and fill in the template. Link the issue it closes with `Closes #N`.

Pull requests are merged with rebase or squash, so `main` keeps a linear history.

The rules for agents working in this repository are in [AGENTS.md](https://github.com/ingvarch/kraken/blob/main/AGENTS.md).

## Code of conduct

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

By contributing, you agree that your contributions are licensed under the [MIT License](https://github.com/ingvarch/kraken/blob/main/LICENSE).
