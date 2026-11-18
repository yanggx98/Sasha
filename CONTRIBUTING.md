# Contributing to Sasha

Thanks for your interest in improving Sasha.

## Reporting issues

Use [GitHub Issues](https://github.com/yanggx98/Sasha/issues). Please include your
Windows version, the application revision, reproduction steps, and the serial
parameters involved. For UI problems, add a screenshot and your display scaling.

## Development setup

You need Git and Rust with 2024 edition support. On Windows, use the MSVC toolchain
with Visual Studio C++ Build Tools and a Windows SDK.

```powershell
git clone https://github.com/yanggx98/Sasha.git
cd Sasha
cargo run --locked
```

## Before opening a pull request

```powershell
cargo check --locked
cargo test --locked
```

- Keep changes focused; unrelated refactors belong in separate pull requests.
- Add or update tests for behavior changes where practical.
- User-visible strings go through `locales/en-US.toml` and `locales/zh-CN.toml`;
  update both.
- Update the README or `docs/` when you change documented behavior.
- Hardware communication and desktop interaction cannot be fully covered by tests.
  Describe how you verified them manually.
- Windows is the validated platform. Reports and fixes for macOS and Linux are welcome
  but note what you tested.

## Commit messages

Use short, imperative messages, preferably in the Conventional Commits style used in
the history, for example `feat(ui): add log search` or `fix(serial): ...`.

## License

By contributing, you agree that your contributions are licensed under
**GPL-3.0-only**, the same as the project.
