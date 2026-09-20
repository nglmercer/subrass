What I would do next, in this order:

| Priority | Next work                                       | Why                                                                                                                                                                                                                                                                                      |
| -------- | ----------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **P0**   | **Fix YCbCr semantics**                         | Completed in the working tree: renderer RGBA stays authored-RGB, all libass matrix metadata values parse, and `convert_ass_rgb` requires an explicit destination `VideoColorSpace`. Raw and host-converted reference layers are tested separately. |
| **P0**   | **Add byte-oriented ASS ingestion**             | Completed in the working tree: `parse_bytes(&[u8])`, renderer/WASM byte constructors, raw event payload retention, mid-event `\fe` decoding, actual non-UTF-8 fixtures, and a dedicated fuzz target. |
| **P1**   | **Finish splitting `compositor.rs`**            | In progress: style resolution, event layout preparation, and the giant compositor test module now have separate ownership; the paint/composition pipeline remains to be separated. |
| **P1**   | **Finish override-tag dispatch split**          | Helpers moved out, but `parse_tag_with_params` and main dispatch still live in the ~1k-line production facade. Split positioning, typography, transforms, effects and resets into tag-family parsers.                                                                                    |
| **P1**   | **Move giant unit-test modules to child files** | Completed for the compositor: `compositor_tests.rs` now owns the ~3,500-line test module and `compositor.rs` is ~1,650 lines. Apply the same boundary review to override tags, font and shaper where it materially improves ownership. |
| **P2**   | **Legacy charset completion**                   | Johab and charset-based font linking remain intentionally unsupported. Resolve these after byte-oriented parsing exists.                                                                                                                                                                 |
| **P2**   | **Benchmarks/performance pass**                 | Once architecture settles, benchmark shaping, blur, clips, drawings, TTC fallback, 1080p/4K rendering and cache behavior before optimizing.                                                                                                                                              |
| **P3**   | **API stabilization**                           | After correctness work, clean public/private boundaries, reduce `pub(crate)` leakage, document stable WASM/native APIs, and prepare for a real versioned release.                                                                                                                        |

For `compositor.rs`, I would specifically aim for something like:

```text
renderer/
  compositor.rs          # orchestration facade plus paint pipeline (next split)
  compositor_tests.rs    # compositor regression/unit tests
  compositor_style.rs    # base/override style resolution
  compositor_layout.rs   # event positioning, segment state, shaping/layout prep
  resolve/
    style.rs             # base + override style resolution
    segment.rs           # segment state
    event.rs             # event-global state
  layout/
    event.rs
    lines.rs
    wrapping.rs
  paint/
    event.rs             # render/composite event
    glyph.rs
    drawing.rs
    decorations.rs
    clipping.rs
```

One thing I **would not do next** is keep splitting files purely by size. The recent modularization is already good enough that the next refactor should follow actual pipeline boundaries. The best architectural target is now to make `Compositor::composite_event_inner` stop being the place where layout, shaping, effects, glyph rasterization, drawing, karaoke and final blending all converge.

Also, I could not verify a GitHub commit-status check for `04e2a600`—the connector reports no combined statuses for that HEAD. The current checkpoint is validated locally after the YCbCr, byte-ingestion, Symbol, and compositor-test moves.

So the immediate sequence I recommend is **regression gates → finish compositor paint split → override-tag dispatch split → charset/font-linking decision → performance/API hardening**.

## Progress recorded during this pass

The following structural extractions are now present without changing the public facade or reference fixtures:

- renderer limits/geometry, font SFNT/collection readers, drawing lexer/paths/raster helpers, effect families, shaper encoding/OpenType/fallback/line-break helpers, and override-tag value/parser helpers are private submodules behind their existing facades;
- parser section dispatch is in `src/parser/sections.rs`;
- UTF-8 BOM handling and the bounded string section scan are in `src/parser/input.rs`, while `AssDocument::parse(&str)` remains unchanged as the stable entry point;
- WASM timestamp validation and serde conversion are in `src/api/serialization.rs`, while existing JS exports remain in `src/api.rs`.
- compositor style resolution and layout preparation are in `src/renderer/compositor_style.rs` and `src/renderer/compositor_layout.rs`;
- compositor tests are in `src/renderer/compositor_tests.rs`, leaving the production compositor facade focused on orchestration and paint.

The structural work is intentionally not marked complete: compositor paint/test ownership, override-tag dispatch, charset/font-linking completion, and the remaining compatibility audits still need work. The next regression checkpoint should include the full local Rust, golden, reference, WASM, browser, Bun, audit, and fuzz matrix available in the environment.

Latest checkpoint: `cargo fmt --all -- --check`, clippy, `cargo test --locked --all-features`, `cargo test --locked --no-default-features`, wasm `cargo check`, golden, and strict reference tests pass locally. `cargo fuzz build parse_ass_bytes` is registered but cannot run on this Windows stable/MSVC environment because cargo-fuzz requires nightly sanitizer flags; the existing README documents Linux/WSL as the supported fuzz environment. Browser/WebDriver and `cargo audit` also remain environment-only gaps documented in the final report.
