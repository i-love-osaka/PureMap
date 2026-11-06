# Contributing

Thanks for your interest in improving PureMap.

## Before opening a pull request

- Keep changes focused and explain user-visible behavior changes.
- Run `cargo fmt --check` and `cargo test --locked`.
- For code changes, also run `cargo clippy --all-targets --locked -- -D warnings`.
- Update the README when changing build steps or user-facing behavior.

GitHub Actions runs formatting, tests, and a release build on macOS, Linux, and
Windows.

## Reporting issues

Include your operating system, Rust version (`rustc --version`), steps to
reproduce the issue, and relevant error output. Do not include private mind-map
documents or other personal data.
