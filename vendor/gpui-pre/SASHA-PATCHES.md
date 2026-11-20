# Sasha patches to gpui-pre 0.3.12

This directory is `gpui-pre` 0.3.12 from crates.io (Apache-2.0, see `LICENSE-APACHE`)
with the changes below, wired in through `[patch.crates-io]` in the root `Cargo.toml`.
Everything else is unmodified.

## `WhiteSpace::BreakAnywhere`

Word wrapping moves a token that does not fit the rest of a line whole to the next
line, leaving a large gap. Log lines often hold long unbreakable tokens (file paths,
`name:line:message` strings), so Sasha wants lines to fill and break between any two
characters.

- `src/style.rs`: new `WhiteSpace::BreakAnywhere` variant.
- `src/styled.rs`: `Styled::whitespace_break_anywhere()`.
- `src/elements/text.rs`: text wraps for `Normal` and `BreakAnywhere`, and passes the
  mode on when shaping.
- `src/text_system.rs`: new `TextSystem::shape_text_with_whitespace`; `shape_text` keeps
  its signature and wraps at word boundaries as before.
- `src/text_system/line_layout.rs`: `compute_wrap_boundaries_with(.., break_anywhere)`
  never remembers a word-boundary candidate, so an overflowing line breaks at the glyph
  that overflowed. The flag is part of the wrapped-layout cache key. Tests added.

To upgrade `gpui-pre`, copy the new release over this directory and reapply the above.
