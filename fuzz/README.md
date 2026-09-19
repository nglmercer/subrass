# Fuzzing (requires nightly Rust + cargo-fuzz)

Pinned toolchain (same as CI `fuzz-smoke`): `nightly-2026-09-17` and
`cargo-fuzz 0.13.2`.

```bash
rustup toolchain install nightly-2026-09-17
cargo install cargo-fuzz --version 0.13.2 --locked
cd fuzz
cargo +nightly-2026-09-17 fuzz run parse_ass -- -max_total_time=60
cargo +nightly-2026-09-17 fuzz run drawing  -- -max_total_time=60
cargo +nightly-2026-09-17 fuzz run render   -- -max_total_time=60
```

Note: `cargo fuzz build` on Windows MSVC fails linking this crate's
`cdylib` artifact (`LNK2001 main`); run fuzzing on Linux (CI) or WSL.

Targets:

- `parse_ass`: `AssDocument::parse` over arbitrary bytes (never panics,
  hangs, or over-allocates; seed corpus in `fuzz/corpus/parse_ass/`).
- `drawing`: `DrawingParser::measure` over hostile command streams
  (subdivision/contour caps stay bounded).
- `render`: full `SubtitleRenderer` frame render over hostile documents
  (extreme positions/clips/rotations/shears/blurs/dimensions degrade to
  empty output or errors, never panic). This path includes legacy charset
  decoding, OpenType shaping/bidi, collection-face selection, nested
  transforms, and glyph/clip allocation limits.

Short smoke runs (build + 20s per target) are CI-gated
(`fuzz-smoke` job); run longer sessions locally for real coverage.
A stable-toolchain, no-panic smoke over hostile values lives in
`tests/robustness.rs`, and a parse corpus without panics in
`tests/corpus.rs`.
