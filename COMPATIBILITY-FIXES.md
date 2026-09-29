# Compatibility fixes and validation

Inspected HEAD and audit baseline: `7c9215eef69e46ea49db1ea74da9fe5762c0dfe8`.
Changes are delivered in the working tree. No claim of full ASS/libass parity
is made. The reference target, dependency versions, configuration, feature
statuses, and intentional differences are in [CONFORMANCE.md](CONFORMANCE.md).

## Confirmed findings and changes

- Collision handling was absent from the active-event render loop. The renderer
  now measures bounded geometry, places eligible events in source order within
  layers, and remeasures/paints one event at a time. It excludes positioned,
  origin, animated, and legacy motion events. Fully faded dialogue still reserves
  space. Semantic tests cover top/middle/bottom alignments, layers, exclusions,
  event start boundaries, and repeated seeking.
- Drawing coverage used one average scale and omitted the text transformation
  path. Drawings now use independent axes and shared shear/rotation/perspective
  semantics, including origin and animation. Non-square shapes, mixed text,
  drawing advance, baseline offsets, clipping, and karaoke are covered.
- `\be` was conflated with `\blur`, and blur affected composited event color.
  They now have independent resolved state, resets, and interpolation. Binomial
  edge filtering and Gaussian filtering operate on bounded coverage masks before
  color, for appropriate fill, outline, and shadow masks. Compatible text glyphs
  and karaoke sweeps combine per style run; inline changes cannot blur neighbors.
  Wide Gaussian masks use a linear-time, variance-matched approximation.
- Initial/repeated hard breaks were dropped; escaped braces and unknown escapes
  were inconsistent. Exact parser regressions now cover these and soft breaks.
  PlayRes presence is retained separately from inferred effective dimensions.
  UTF-16 BOM decoding handles both endiannesses and rejects malformed units and
  surrogates. Johab remains explicitly unsupported with style/tag diagnostics.
- Banner/Scroll were incorrectly exempted as unsupported by libass. Investigation
  confirmed libass implements them; timing quantization now happens before
  output scaling, and both historical fixtures plus targeted motion samples gate.
- Active render fuzzing exposed an additional existing infinite loop: an unclosed
  override brace left the encoding scanner cursor unchanged in both string and
  byte paths. The scanner now consumes the tail, preserves escaped syntax, and
  terminates. Exact decoder tests, a browser test, and the original mutated seed
  protect this fix. The retained seed is
  [unterminated-override.ass](fuzz/corpus/render/unterminated-override.ass).

## Regression assets and provenance

The strict report is derived from actual test output by
[report.py](tests/reference/report.py), rather than manually maintained counts:
**180 gated passes** (95 historical FFmpeg, 85 direct libass 0.17.5), **5 measured
intentional divergences**, **5 host-converted artifacts**, **0 pending**,
**0 gated failures**. See [per-frame results](tests/reference/results.md).

New samples cover two output resolutions, animation interval boundaries,
collision eligibility and fade reservations, all drawing transform axes,
independent/combined blur, inline resets, karaoke, and legacy motion.
The direct oracle uses the committed DejaVu font with system discovery disabled.
The historical FFmpeg version is reconciled to its recorded essentials product
string; its unrecorded libass dependencies remain explicitly unknown.

All historical references and all ASS self-golden inputs are unchanged.
Only the self-golden `border-shadow.rgba` changed, after independent historical
reference IoU improved from 0.891 to 0.953 and mean error from 18.94 to 11.11.
Thresholds were not weakened. New direct references come from the pinned libass
oracle, not subrass output, and use lossless RGBA run-length encoding.

## Changed file groups

- Parser/types: `src/parser/{input,mod,script_info}.rs`,
  `src/types/{override_tag,script_info,effect}.rs`.
- Measurement/placement: `src/renderer/{mod,compositor,compositor_layout}.rs`,
  `src/renderer/layout/{lines,wrapping}.rs`.
- Drawing/blur: resolved state, style/transform code, drawing coverage,
  `src/renderer/paint/{text,drawing,glyph,transform,event,clipping,mod}.rs`,
  karaoke and outline helpers. Effect work is clipped before dilation where it
  cannot contribute; existing allocation/geometry limits are preserved.
- Encoding scanner: `src/renderer/shaper.rs` and `shaper/encoding.rs`.
- Regression/reference assets: `tests/compatibility.rs`, `tests/web.rs`,
  compositor and mask unit tests, `tests/reference.rs`, direct fixture scripts,
  compressed frames, generator, provenance, and generated results.
- Delivery/CI: this report, `CONFORMANCE.md`, `readme.md`, `AUDIT_FIXES.md`,
  historical generator/provenance, fuzz target/README/seeds, and CI.

## Validation on this host

Commands below were run from the repository root unless stated otherwise.
Rust is `1.96.0 (ac68faa20 2026-05-25)`. No dependency versions were changed.
Native counts include all integration tests; the existing benchmark is ignored.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Pass |
| `cargo fmt --manifest-path fuzz/Cargo.toml -- --check` | Pass |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | Pass |
| `cargo test --locked --all-features` | 469 passed, 1 benchmark ignored |
| `cargo test --locked --no-default-features` | 468 passed, 1 benchmark ignored |
| `cargo test --locked` | 468 passed, 1 benchmark ignored |
| `cargo test --locked --no-default-features --features system-fonts` | 469 passed, 1 benchmark ignored |
| `SUBRASS_STRICT_REFERENCES=1 cargo test --locked --test reference -- --nocapture` | 9 tests pass; 180 frame gates pass, no pending/failures |
| `cargo check --target wasm32-unknown-unknown --tests --all-features --locked` | Pass |
| `wasm-pack test --headless --chrome --chromedriver /tmp/subrass-reference/chromedriver-linux64/chromedriver` | 40 browser tests pass |
| `wasm-pack test --headless --firefox` | 40 browser tests pass |
| `wasm-pack build --target web --out-dir pkg` | Pass |
| `bun run typecheck` | Pass |
| `bun test` | 41 tests, 192 assertions pass |
| `cargo audit --deny warnings` | Pass; 81 locked dependencies, 1,277 advisories; existing documented unmaintained `ttf-parser` advisory exception unchanged |
| `python3 tests/compatibility/libass.py --probe-retention` | Reproduces 794 differing pixels between libass playback and fresh seek |

Chrome used `WASM_BINDGEN_TEST_WEBDRIVER_JSON=/tmp/subrass-reference/webdriver.json`
with Playwright Chromium 153.0.8010.12 and matching ChromeDriver. Its JSON was:

```json
{"goog:chromeOptions":{"binary":"/home/meme/.cache/ms-playwright/chromium-1243/chrome-linux64/chrome","args":["--no-sandbox"]}}
```

Browser/build tooling was wasm-pack 0.15.0. Local Bun was 1.4.0-canary.1;
CI requests 1.4.0. Test logs were saved under `/tmp/subrass-reference/`.

Fuzz commands:

```sh
cargo +nightly fuzz build --fuzz-dir fuzz
cargo +nightly fuzz run parse_ass --fuzz-dir fuzz -- -runs=10000 -max_total_time=20 -timeout=10
cargo +nightly fuzz run parse_ass_bytes --fuzz-dir fuzz -- -runs=10000 -max_total_time=20 -timeout=10
cargo +nightly fuzz run drawing --fuzz-dir fuzz -- -runs=10000 -max_total_time=20 -timeout=10
cargo +nightly fuzz run render --fuzz-dir fuzz fuzz/corpus/render/unterminated-override.ass -- -runs=1 -timeout=10
cargo +nightly fuzz run render --fuzz-dir fuzz -- -runs=10000 -max_total_time=20 -timeout=10
```

Final parser smoke: 10,000 runs in 3 seconds; byte parser: 10,000 in 2 seconds;
drawing: 10,000 in less than one reported second. The active render smoke initially stalled at an
unclosed brace, and the saved input independently timed out with `-timeout=10`.
After the fix, the original input executed in 12 ms. The final active render smoke completed 10,000 runs in 14 seconds with no
crash or timeout. Fuzzing is a bounded smoke, not a
proof that every costly input is handled. Local nightly was rustc 1.99.0-nightly
`0e29c21d9 (2026-07-21)` / cargo `3efb1f477 (2026-07-17)`;
CI's nightly-2026-09-17 was not installed locally.

CI now installs the WASM target before its native job checks that target,
includes the raw-byte fuzz target, and enforces a per-input fuzz timeout.
The render harness now chooses an active event interval; deriving a timestamp
from an ASS header previously sampled ordinary seeds long after they ended.

## Unavailable checks and remaining limitations

Windows native/browser/fuzz execution, hosted GitHub Actions, and the historical
Windows FFmpeg/PowerShell generator were not run on this Linux host. PowerShell
is unavailable. The existing benchmark was not explicitly enabled. New native
libass references and Linux browser tests were executed directly.

Remaining measured divergences are font fallback and four Unicode wrapping
fixtures. Collision placement intentionally depends only on the active frame;
it does not reproduce libass's retained playback collision slots. Run the
retention probe above to reproduce that difference. Fresh-frame placement and
seek determinism are tested separately.

Drawing/text perspective with differing LayoutRes and PlayRes, post-rotation
aspect correction, exact stroker/rasterizer/kernel quantization, rotated/blurred
BorderStyle 3 boxes, optional Scroll/Banner fade edges, and exhaustive curved
vector clips remain partial. Extremely large blur runs may use the bounded
per-glyph fallback. Johab, UTF-32, and BOM-less UTF-16 remain unsupported.
Explicit invalid PlayRes and malformed UTF-16 remain parser errors. These limits
are documented separately from rendered/reference-tested features.
