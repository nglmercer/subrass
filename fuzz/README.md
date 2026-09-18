# Fuzzing (requires nightly Rust + cargo-fuzz)

```bash
cargo install cargo-fuzz
cd fuzz
cargo fuzz run parse_ass -- -max_total_time=60
cargo fuzz run drawing  -- -max_total_time=60
cargo fuzz run render   -- -max_total_time=60
```

Targets:

- `parse_ass`: `AssDocument::parse` over arbitrary bytes (never panics,
  hangs, or over-allocates; seed corpus in `fuzz/corpus/parse_ass/`).
- `drawing`: `DrawingParser::measure` over hostile command streams
  (subdivision/contour caps stay bounded).
- `render`: full `SubtitleRenderer` frame render over hostile documents
  (extreme positions/clips/rotations/shears/blurs/dimensions degrade to
  empty output or errors, never panic).

Short smoke runs (build + 20s per target) are CI-gated
(`fuzz-smoke` job); run longer sessions locally for real coverage.
A stable-toolchain, no-panic smoke over hostile values lives in
`tests/robustness.rs`, and a parse corpus without panics in
`tests/corpus.rs`.
