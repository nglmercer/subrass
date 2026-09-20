# Rendering benchmarks

The ignored integration benchmark measures release-mode frame latency for
plain 1080p text, many simultaneous 1440p events, a 4K effects/drawing case,
and mixed Arabic/Indic/Hebrew karaoke shaping.

Run it reproducibly with:

```text
cargo test --release --test benchmarks -- --ignored --nocapture --test-threads=1
```

It prints p50 and p95 latency plus every sample. Run on a quiet machine and
record the Rust toolchain, CPU, OS, and feature set with the result. The
production glyph cache is bounded and deterministic; cache hit-rate and
allocation counters are intentionally not part of the public API yet, so the
benchmark does not pretend to measure those internal counters.
