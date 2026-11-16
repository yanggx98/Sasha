# Vendored GPUI caret extension

`gpui-base/` is the Apache-2.0 licensed crates.io `gpui-base` 0.9.1 source.
Upstream repository: https://github.com/longbridge/gpui-kit
Published source revision: `39ab849a93b5f15507b969164d49891db8d5b470`, `crates/base`.
The original license is retained in `gpui-base/LICENSE-APACHE`.

The root Cargo manifest uses `[patch.crates-io]` to ensure all consumers use this
same source. No Cargo registry cache modifications are required.

## Local changes

The focused upstream diff is recorded in `gpui-base-caret.patch` beside this file.

- `src/input/base/state.rs`: add per-input `caret_style(width, gap)`. The default
  remains a 1px caret with no gap. Include the caret span in retained scrolling.
- `src/input/base/element.rs`: use the configured width and gap in native caret
  geometry, scroll clearance, right-edge clamping and wrap margins.
- The original caret painter, blink timer, focus handling and IME integration
  remain in use. Ordinary inputs retain their original appearance.

Sasha configures only its send textarea with `caret_style(px(4.), px(2.))`.
There is no application-side caret overlay or second blink timer.

## Validation and upgrades

Run `cargo check --offline` and `cargo test --offline` at the repository root.
`src/ui/caret_tests.rs` exercises default-vs-block geometry and Unicode wrapping
and scrolling through the real input component. GPUI's test-support backend is
a development-only dependency. Also verify blinking, typing, cursor movement,
selection, focus changes and IME composition in the Windows desktop app.

For an upstream upgrade, compare the two modified files against the new release,
reapply the extension if still needed, and rerun the tests and desktop checks.
Once upstream offers equivalent per-input configuration, remove this patch and
switch the send textarea to the upstream API.
