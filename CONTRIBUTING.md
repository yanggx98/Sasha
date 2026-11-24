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

## Publishing a release

The in-app updater reads GitHub Releases. Tag releases as `vX.Y.Z` (or `vX.Y.Z-pre`),
matching `version` in `Cargo.toml`, and attach the Windows build from
`cargo build --release --locked` as `sasha-windows-x86_64.exe`. GitHub records a SHA-256
digest for uploaded assets, which the updater requires; alternatively upload a
`sasha-windows-x86_64.exe.sha256` file. Releases without a verifiable asset still
notify users but link to the release page instead of installing.

## Versioning

Sasha follows [Semantic Versioning](https://semver.org/) in its `0.x` phase, with a
`-b` (beta) suffix until the features and platforms are validated, e.g. `0.1.9-b`.

- **MINOR** (`0.X.0`): new features, or incompatible changes to behavior or config.
- **PATCH** (`0.1.X`): fixes, docs and CI only, no new features.
- Drop `-b` only when the release is considered stable.
- Any tag containing `-` is published as a pre-release.
- Bump `version` in `Cargo.toml` only when you are about to release, in its own
  `chore: bump version to X` commit, then tag `vX` on that commit. Don't leave a
  bumped version without a matching tag and release.

## License

By contributing, you agree that your contributions are licensed under
**GPL-3.0-only**, the same as the project.

Pushing the tag runs the release workflow, which builds `sasha-windows-x86_64.exe` and
`sasha-windows-x86_64.zip` (the same exe, zipped for manual download),
`sasha-linux-x86_64.tar.gz` and `sasha-macos-aarch64.tar.gz` (each with a `.sha256`
file) and publishes them together. The Windows build must pass; a Linux or macOS
failure only leaves that package out. The in-app updater installs the Windows asset
only, and it needs the bare `.exe` asset to stay (it replaces the running executable
with the downloaded file); other platforms are directed to the release page.
