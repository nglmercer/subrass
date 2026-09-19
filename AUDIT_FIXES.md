# Audit Remediation Log

Remediation of the 100-item audit in `temp_plan.md`, grouped by phase,
plus follow-up review passes. All Rust behavior changes carry regression
tests; run `cargo test`. `CONFORMANCE.md` is the test-backed
compatibility statement; this file is the history of how it got there.

Applies to HEAD after `ff336a4` (second-pass remediation), the
follow-up passes described below, and the precedence/clip/server pass
at the bottom (first-wins semantics, libass clip state, dev-server
rewrite, attachment budgets, 13 new reference fixtures).

## Phase 1 — Safety invariants (#1–3, #70–75, #95)

- **#1 Render buffer safety** (`src/renderer/buffer.rs`): `RenderBuffer::new`/`resize`
  return `Result<_, RenderError>` with checked `w*h*4` arithmetic, `MAX_DIMENSION`
  (16384), and `MAX_BUFFER_PIXELS` (64M). WASM sizing APIs surface errors to JS.
- **#2 `fill_rect` bounds** (`src/renderer/buffer.rs`): i64 intersection math;
  negative sizes, `i32::MIN/MAX` origins, and overflowing `x+w` are safe no-ops.
- **#3 Clip helpers** (`src/renderer/effects.rs`): corners normalized (min/max),
  intersected with the buffer, empty/offscreen handled (clip-all-clears,
  inverse-clip-offscreen-untouched); `apply_shadow`/`apply_outline` use
  saturating offsets, checked pixel conversion, and clamped radii.
- **#70 Resource limits**: buffer/dimension caps, `MAX_BLUR_RADIUS` (128),
  `MAX_OUTLINE_RADIUS` (128), `MAX_EVENTS` (100k), `MAX_STYLES` (10k),
  `MAX_ATTACHMENTS` (256), `MAX_ATTACHMENT_BYTES` (64 MiB),
  `MAX_DRAWING_COMMANDS` (100k), `MAX_DRAWING_POINTS` (1M),
  `MAX_GLYPH_BITMAP_PIXELS` (16M). High but finite; exceeding them errors.
- **#71 Projective transform** (`src/renderer/buffer.rs`): rejects non-finite
  matrices/perspective, guards the near-zero perspective denominator, clamps the
  output size, validates every inverse-mapped sample; degenerate input yields an
  empty bitmap.
- **#72 Drawing rasterizer** (`src/renderer/drawing.rs`): `total_cmp` ordering
  (no `partial_cmp().unwrap()`), finite-coordinate validation, buffer-clamped
  spans before looping, clamped scanline ranges.
- **#73 Override numerics** (`src/types/override_tag.rs`): every parsed tag is
  finite-checked (`all_finite`); `NaN`/`inf` payloads become `Unknown` and never
  reach layout.
- **#74 Blur radius** (`src/renderer/effects.rs`, `buffer.rs`): non-finite and
  negative radii ignored; `u32` radii clamped to `MAX_BLUR_RADIUS`.
- **#75 Fade underflow** (`src/renderer/effects.rs`): `saturating_sub` for the
  fade-out start; overlapping fade-in/out take the minimum (darker wins).
- **#95 Allocation intent**: `vec![0; len]` zero-fill is intentional
  (transparent black); bounded by the same caps as #1.

## Phase 2 — Core behavior (#4–17, #76, #81, #86–88)

- **#4 Glyph cache font collision** (`src/renderer/glyph_cache.rs`): the cache
  key includes the stable `font_id`; same glyph ID in two fonts no longer shares
  rasters.
- **#5 Resize consistency** (`src/renderer/mod.rs`): `resize` and
  `set_video_size` update buffer + video + canvas transfer sizes together and
  return `Result`; `frame_size` reads the buffer (source of truth).
- **#6 WASM time validation** (`src/api.rs`): `validate_millis` rejects NaN,
  infinities, negatives, and out-of-range values in `get_events_at_time`,
  `render_frame`, and `ms_to_ass_time`.
- **#7 Time parsing** (`src/types/time.rs`): `Time::try_new` rejects minutes > 59,
  seconds > 59, centiseconds > 99; `FromStr` uses it (`0:99:99.99` errors).
  `Time::new` remains as the normalizing helper.
- **#8 Events Format** (`src/types/event.rs`, `src/parser/event.rs`): full
  column mapping — reordered columns, unknown columns ignored, `Text` must be
  last, missing required columns error, SSA `Marked` accepted, keywords
  case-insensitive.
- **#9 Strict numbers**: Layer/Margin fields must parse (`unwrap_or(0)` removed);
  empty values default, malformed values error.
- **#10 Strict styles** (`src/parser/style.rs`, `src/types/style.rs`): malformed
  non-empty values error (incl. non-finite floats); empty values keep defaults;
  style field-count mismatches error; `MAX_STYLES` cap; case-insensitive
  keywords.
- **#11 ScriptInfo validation** (`src/types/script_info.rs`): `set_field`
  returns `Result`; bad `PlayRes` (incl. 0), `WrapStyle` (> 3), `ScriptType`,
  `YCbCr Matrix`, `ScaledBorderAndShadow` produce line-numbered errors.
- **#12 Complex fade** (`src/renderer/compositor.rs`): `\fade` values treated as
  ASS transparency (`opacity = 1 - a/255`) via `complex_fade_opacity`; zero and
  inverted ranges saturate instead of dividing by zero.
- **#13 Transform accel**: corrected to `t^accel` (was `1-(1-t)^accel`);
  non-finite accel falls back to linear; result clamped to [0, 1].
- **#14 Karaoke `\k`**: secondary before the syllable, primary from the exact
  start instant (was: at end). Pure helper `karaoke_is_primary`.
- **#15 Karaoke outline `\ko`**: secondary fill + outline suppressed *before*
  the syllable start, primary fill + outline from the exact start instant
  (corrected from an earlier reversed implementation). Helper
  `karaoke_outline_suppressed`.
- **#16 Karaoke ordering**: `\K`/`\kf` sweep edges clamp to [0, 1], split
  within glyph bitmaps (a single glyph holds primary and secondary pixels
  during the sweep); `\ko` and sweep combine per documented rules.
- **#17 `\n` vs `\N`**: `\N` always breaks; `\n` is a space except in wrap
  mode 2, in segmentation (`parse_text_segments_with_wrap`), wrapping, and
  clean-text extraction.
- **#76/#81 Alpha terminology** (`src/types/color.rs`): `alpha` documented as
  ASS transparency; added `transparency()`, `opacity()`,
  `to_straight_rgba()`, `to_ass_components()`, `from_standard_rgba()`.
- **#86 Error context**: strict parsers name the field and quote the value;
  line numbers included (see #92).
- **#87 `validate_ass`**: now meaningful — strict parsing fails on malformed
  values; semantics documented on the function.
- **#88 Empty docs**: `[Script Info]`-only and empty-but-valid documents parse;
  rendering an empty document yields transparent frames (tested).

## Phase 3 — ASS compatibility (#18–36, #82, #83)

- **#18 Wrap modes** (`src/renderer/compositor.rs`): mode 0 = smart
  (raggedness-minimizing DP, balanced/top-widest), 1 = greedy EOL, 2 = none,
  3 = greedy bottom-widest. Mode 0 is an approximation of VSFilter (documented).
- **#19 Legacy `\a`**: split from `\an`; SSA numbering converted via shared
  `ssa_alignment_to_ass` (moved to `types::style`, reused by the style parser).
- **#20 `\rStyleName`**: `Reset(Option<String>)` with style-table lookup;
  unknown names fall back to the event style.
- **#21 Global vs segment tags**: `OverrideTag::is_line_global` classifies
  `\pos \move \org \clip \iclip \fad \fade \an \a`; `resolve_style` scans all
  segments in textual order (first wins per libass, except rect clips and
  `\q`), so placement after the first segment no longer drops them.
- **#22 X/Y borders**: `outline_x/outline_y` state, `\xbord \ybord` (and `\t`)
  support, elliptical `apply_outline_xy` with degenerate-axis fallback.
- **#23 X/Y shadows**: `shadow_x/shadow_y` (signed, incl. negatives),
  `\xshad \yshad` (and `\t`) support; active when either is non-zero.
- **#24 `\fax \fay`**: parsed, transformed (`\t`-able), and rendered as
  pre-rotation shear in glyph-local coordinates before the Z/X/Y rotation
  matrix — matching libass `calc_transform_matrix`, which builds the shear
  basis first and composes rotations over it. Bounded, clamped ±8.
- **#25 `ScaledBorderAndShadow`**: threaded from ScriptInfo; `no` maps script
  units 1:1 to video pixels for borders/shadows (default `yes` scales).
- **#26 `LayoutResX/Y`**: parsed and validated; unused with a documented reason
  (ASS-2 draft fields; PlayRes is authoritative).
- **#27 YCbCr matrix**: parsed and validated; documented as not applicable to
  this RGB pipeline.
- **#28 Drawing scale**: corrected to `res_scale / 2^(mode-1)` (was multiplied);
  `\p2` units render half the size of `\p1` units.
- **#29 Drawing commands**: `n` continues the outline without closing (vs `m`
  which splits); `s` cubic B-spline evaluation; `p` spline extension; `c`
  spline+outline close; all covered by tests.
- **#30 `\pbo`**: parsed (`DrawingBaseline`), applied as a baseline Y offset
  (positive moves the drawing up), in script units.
- **#31/#32 Mixed drawing/text**: per-segment drawing state (any `\pN>0`);
  drawing segments render inline at the pen with measured bounds and advance;
  wrapping never splits drawing runs (verbatim passthrough).
- **#33 Vector clips**: `\clip([scale,]drawing)` / `\iclip(...)` parsed
  (rect-vs-vector disambiguated; malformed rectangles rejected, not mistaken
  for vectors) and rendered as alpha masks in script coordinates.
- **#34 Clip/blur order**: blur runs **before** clipping so blurred pixels
  cannot bleed outside the clip region. Re-verified against libass
  `ass_render.c` ("convert glyphs to bitmaps, combine them, apply blur",
  clips applied at blend time): the order matches libass; only the blur
  granularity differs (per combined-bitmap run vs whole event buffer).
- **#35 Layout measurement**: two-pass layout — every segment resolved (incl.
  `\t` at the frame time) and shaped with its own style; alignment, `\pos`,
  `\move`, rotation origins, and opaque boxes use block metrics from the same
  dimensions used for rendering. Karaoke recolor applies post-layout, with
  layout-aware (shaped-width) sweep edges.
- **#36 Shaper measurement**: per-line trailing-spacing rule (no more global
  `max_x` corruption on newlines); `measure_text` returns the widest line.
- **#82 Opaque box**: uses layout block metrics; back-color alpha honored;
  margins saturate. Single-line matches libass (IoU 0.992); multi-line covers
  the whole block while libass draws per-line boxes (known divergence, has a
  dedicated fixture).
- **#83 `\r` semantics**: resets segment state (fonts, colors, border/shadow,
  rotation, karaoke, **drawing mode**, `\pbo`) to the event/named style while
  preserving line-global state (position, move, origin, clips, fades) plus
  alignment (libass `ass_reset_render_context` never touches alignment).
  `\r` exits drawing mode (`\p` is not line-global); libass keeps drawing
  across `\r`, which stays a documented intentional difference.

## Phase 4 — Fonts, attachments, colors (#37–45, #93, #94, #97)

- **#37 Deterministic selection** (`src/renderer/font.rs`): exact → family →
  normalized → fallback precedence, all in load order; no `HashMap` iteration
  in matching paths.
- **#38 Metadata detection**: sfnt `name` (IDs 1/4/16), `OS/2.fsSelection`, and
  `head.macStyle` parsing (bounds-checked, no new dependencies); declared
  family names registered as aliases; filename heuristics only as fallback.
- **#39 Faux bold/italic**: `FontMatch` reports `faux_bold/faux_italic` (true
  only when the face lacks the style); the cache keys on them so real-bold and
  faux-bold never share rasters; faux italic implemented as a 12° shear.
- **#40 Attachment parsing** (`src/parser/attachment.rs`): section-aware
  headers (`fontname:` in `[Fonts]`, `filename:` in `[Graphics]`,
  case-insensitive; wrong-section headers error); trailing `\r` handled.
- **#41 Decode errors**: malformed uuencode payloads are line-numbered errors
  (never silent empty files); per-attachment and count caps enforced; payload
  alphabet validated.
- **#42 Embedded fonts**: exposed to JS (`get_attachment_count/name/kind/data`);
  `[Fonts]` attachments are additionally best-effort auto-loaded by the
  renderer (failures surface via `warnings()`); manual `load_font` remains.
- **#43 Attachments in docs**: parser tree + matrix updated.
- **#44 Color helpers**: conversion directions explicit; `to_hex` emits valid
  CSS `#RRGGBBAA` (opacity slot).
- **#45 CSS hex**: `#RRGGBBAA` (was mis-parsed as `#AARRGGBB` with raw alpha).
- **#93 Unexported surface**: `FontMatch` documents the matching contract.
- **#94 Cache keys**: font id + size bits + faux flags; deterministic LRU
  eviction (tick-ordered, protected from evicting the just-inserted glyph).
- **#97 Shaping tests**: shaping is covered by layout/render tests; complex
  shaping (HarfBuzz) remains out of scope and is documented. Per-glyph
  fallback across already-loaded faces **is** implemented.

## Phase 5 — Worker/demo/server (#46–58)

- **#46 Serialization**: all `AssDoc` getters return `Result<JsValue, JsError>`
  via `serde_wasm_bindgen` (no `unwrap_or_default`); TS call sites unchanged
  (same signatures, throwing).
- **#47 Worker init hangs**: `readyReject` stored; `onerror`/`onmessageerror`/
  worker `fatal` reject the promise; `dispose` settles waiters.
- **#48 `loadAss` rejection**: `{resolve, reject}` waiters keyed by request ID;
  worker exceptions reject instead of hanging.
- **#49 Request IDs**: every worker message carries `requestId`, echoed in
  responses; FIFO assumptions removed.
- **#50 Backpressure**: one render in flight; newer frames while busy collapse
  to the latest pending request.
- **#51 Object URLs**: previous video blob URL revoked on replacement.
- **#52 Debug output**: central `demo/shared/debug.ts` (off by default;
  `?debug` or localStorage opt-in); all `DEBUG = true` sites removed.
- **#53 Stale docs**: worker-init comments corrected (both threads init).
- **#54 MIME handling**: font/subtitle/image MIME map in `server.ts`.
- **#55 Traversal**: repeated-decode + root-containment (`resolveContained`)
  for `/pkg/`, `/fonts/`, and `.ts` routes.
- **#56 Demo errors**: renderer calls wrapped in try/catch (throwing WASM APIs).
- **#57 `Effect` field**: parsed, exposed, and **rendered**: `Banner` and
  `Scroll up/down` with VSFilter timing, band clip, and edge fadeaway.
  libass ignores these effects (renders static text), so their reference
  fixtures are known-divergent, not gated.
- **#58 Support matrix**: unsupported-but-parsed tags documented in README.

## Phase 6 — Quality gates (#59–69, #77–80, #84, #85, #89–92, #96, #98)

- **#59 README**: rewritten — accurate tree (incl. `attachment.rs`), verified
  pipeline, corrected claims (karaoke colors, wrap, blur/clip order).
- **#60 WASM tests** (`tests/web.rs`): semantic assertions (counts, texts,
  sort order, summary fields, renderer frame bytes, error paths); fixed the
  wrong "active at 2000ms" comment (both events active).
- **#61 CI** (`.github/workflows/ci.yml`): fmt, clippy `-D warnings`, native
  tests (ubuntu + windows), wasm32 check, browser tests, `cargo audit`,
  demo typecheck, fuzz smoke.
- **#62 Reference tests: done.** `tests/reference.rs` compares subrass output
  against libass-rendered frames (ffmpeg `ass` filter, pinned build recorded
  in `tests/reference/provenance.json`). 48/48 gated fixtures pass on
  structural gates (bbox IoU ≥ 0.70, ink ratio 0.5–2.0, block mean error ≤ 25,
  hard-error fraction ≤ 0.15); 3 legacy/fallback fixtures are measured but
  known-divergent; 0 pending (every manifest fixture has a libass frame, and
  `SUBRASS_STRICT_REFERENCES=1` in CI fails the gate on any future pending).
- **#63 Fuzz targets: done.** `fuzz/` workspace (`parse_ass`, `drawing`,
  `render`) plus a CI `fuzz-smoke` job (nightly, builds all targets, runs
  each briefly). Longer sessions run locally; see `fuzz/README.md`.
- **#64 Golden tests: done.** `tests/golden.rs` renders 51 fixtures and
  compares byte-exact against stored raw-RGBA expectations
  (`UPDATE_GOLDENS=1` regenerates; never automatic).
- **#65 Dependency audit**: CI job added; `cargo audit` status recorded below.
- **#66 Toolchain**: `rust-toolchain.toml` pins `1.96.0` + components, and CI
  passes the same version explicitly (`toolchain: "1.96.0"`, since the
  setup action does not read the repo file itself).
- **#67 Package scripts**: `build/dev/start/typecheck/test` in package.json.
- **#68 fmt**: `cargo fmt` clean.
- **#69 clippy**: `cargo clippy --all-targets` clean (was 9 warnings).
- **#77 Layer sort**: stable `sort_by_key` kept + regression test.
- **#78 Comment filtering**: render path filters non-dialogue before styling.
- **#79 Per-frame clones**: render collects `&Event` refs (no clone).
- **#80 Docs comments**: code comments verified against behavior.
- **#84 Tag case-sensitivity**: `k/K` preserved; no indiscriminate lowercasing;
  greedy-name splitting (`\rStyle`, `\fnArial`) tested.
- **#85 Malformed vs unknown**: malformed known tags preserved as `Unknown`
  (never half-applied, never silently dropped).
- **#89 Duplicate sections**: styles/events append (ScriptInfo overwrites).
- **#90 Unknown sections**: verified isolated (regression test).
- **#91 Case sensitivity**: section names + `Format:`/`Style:`/`Dialogue:`/
  `Comment:`/`fontname:`/`filename:` case-insensitive (tested).
- **#92 Line numbers**: off-by-one fixed (content starts at header+1); exact
  line asserted in tests.
- **#96 Matrix**: README support matrix (Supported/Partial/Parsed).
- **#98 Toolchain pin**: see #66. `build.sh` also enforces the pinned
  `wasm-pack 0.15.0` (fails with an install hint when a different version
  is present) instead of silently using whatever is installed.

## Follow-up pass (review after `ff336a4`)

- **Relative `\fs+N/-N`** (`src/types/override_tag.rs`,
  `src/renderer/compositor.rs`): libass scales the *current* size by
  `(1 + delta/10)` when the parameter starts with `+`/`-`, and by
  `(1 + progress * delta/10)` inside `\t`. The old code treated the sign
  as part of an absolute number. New representation:
  `FontSize(f64)` (absolute), `FontSizeRelative(f64)` (delta), and
  `FontSizeReset` (bare `\fs`); non-positive results reset to the event
  style size, matching libass. Covered by parser tests
  (`test_fs_relative_parsing_matches_libass`), resolution tests
  (`test_relative_fs_*`), a render test
  (`test_degenerate_font_size_resets_to_style_safely`), the updated
  `tests/robustness.rs` geometry test, and the `relative-fs` golden +
  reference fixture.
- **Combination fixtures** (`tests/golden.rs`): added `relative-fs`,
  `shear-rotation`, `karaoke-kf-frz/frx/fax/fay/fry/combined`,
  `karaoke-early/late`, `karaoke-kf-early/late`, `relative-fs-early/late`,
  and `opaque-box-multiline` (the karaoke fixtures are centered so the
  transformed sweep stays fully on-frame; each shows both sweep colors).
  All are golden-covered and gated against generated libass frames
  (`tests/reference/gen_references.ps1`); `opaque-box-multiline` gates
  (IoU 1.000, per-line boxes like libass) instead of diverging.
- **`\K/\kf` under transforms**: proportional across the transformed
  bitmap (exact when unrotated); gated against libass by the
  `karaoke-kf-frz/frx/fax/fay/fry/combined` fixtures (IoU 0.918–0.963).
- **Shear order**: kept pre-rotation shear — verified against libass
  `calc_transform_matrix`, which builds the shear basis first and
  composes Z/X/Y rotations over it. The earlier "move shear after
  rotation" note was withdrawn; `shear-rotation` covers the combined
  case.
- **`\fe`**: parses, resolves, and resets; charset/encoding remapping is
  still not performed (labeled `Partial` in `CONFORMANCE.md`).
- **Complex shaping**: still unsupported (LTR `ab_glyph` only; no
  HarfBuzz, RTL, ligatures, Indic/Arabic forms), accurately documented
  in README and `CONFORMANCE.md`.
- **Build/CI pinning**: `build.sh` enforces `wasm-pack 0.15.0`; CI
  passes `toolchain: "1.96.0"` explicitly in every Rust job; new
  `fuzz-smoke` CI job builds and briefly runs all three fuzz targets on
  nightly.
- **This log**: rewritten to remove stale "Remaining" entries and old
  claims (reference/fuzz/golden done; Effect rendered; `\r` exits
  drawing mode; current counts below).

## Precedence/clip/server pass (after `3c463f3`)

- **First-wins precedence** (`src/types/override_tag.rs`,
  `src/renderer/compositor.rs`): `\pos`/`\move` share one first-wins slot
  (libass `EVENT_POSITIONED`), first `\org` wins (`have_origin`), first
  `\fad`/`\fade` wins (`PARSED_FADE`), first `\an`/`\a` wins (`PARSED_A`,
  with the `\a4`/`\a8`→`\a5` quirk and bare/out-of-range style fallback
  via `AlignmentReset`). Verified against libass `ass_parse.c`/`ass_render.c`
  and empirically: `{\pos…\move…}X` ≡ `{\pos…}X` byte-identical under
  ffmpeg libass. Old last-wins tests rewritten; resolve- and render-level
  coverage in `test_shared_slots_first_tag_wins`,
  `test_pos_move_first_wins_both_orders`,
  `test_fad_fade_first_wins_both_orders`,
  `test_alignment_first_tag_applies_event_wide`, `test_org_first_wins`.
- **`\r` preserves alignment**: libass `ass_reset_render_context` never
  touches alignment, so the first `\an`/`\a` survives resets (verified:
  `{\an7}A{\r}B` ≡ `{\an7}AB` byte-identical under libass).
  `test_reset_keeps_first_alignment_and_ignores_later` locks this in.
- **Clip semantics** (`apply_single_tag`): rect and vector are separate
  state — later rect coordinates replace earlier ones, `\clip` vs `\iclip`
  flips the rect mode, the first vector clip is retained across forms,
  and both render. `test_clip_libass_rect_vector_semantics` covers all
  eight rect/vector/normal/inverse combinations plus render coexistence.
- **Precedence fixtures** (`tests/golden.rs` + libass frames): 13 new
  gated fixtures — `pos-before-move`, `move-before-pos`, `pos-pos`,
  `move-move`, `an-an`, `a-an`, `an-a`, `fad-fade`, `fade-fad`,
  `org-org`, `rect-vector-clip`, `vector-rect-clip`, `vector-vector-clip`
  (IoU 0.849–0.988, all thresholds unweakened). Pending stays 0.
- **Dev-server rewrite** (`server.ts`, `tests/server.test.ts`): canonical
  `/basic`→`/basic/`, `/worker`→`/worker/` 301 redirects, generic contained
  demo static handler (`.ts` transpiled, correct MIME per extension),
  `/pkg/*` + `/fonts/*` from their own roots, GET/HEAD only (405 otherwise),
  bodiless HEAD with `Content-Length`, strict `PORT` validation
  (`RangeError` on empty/`NaN`/negatives/decimals/out-of-range).
  17 HTTP-level tests (ephemeral port + raw-socket traversal vectors).
- **Attachment budgets** (`src/parser/attachment.rs`, `src/parser/mod.rs`):
  the remaining document count/byte budget is passed into section parsing
  (`parse_attachments_with_budget`), so over-budget data is rejected while
  decoding — count on the exceeding header, encoded growth as soon as its
  implied size exceeds room, exact decoded size before output allocation
  (pure `decoded_len_for`) — instead of after allocating hundreds of MiB.
- **Divergence review**: blur-before-clip re-verified to match libass order
  (granularity differs only); underline/strikeout bars documented as
  axis-aligned under all transforms; no-system-font-discovery and
  `\r`-exits-drawing (libass keeps drawing, probed empirically) added to
  `CONFORMANCE.md`; complex shaping deferred with an explicit
  rustybuzz+bidi roadmap rather than a half-integration.
- **Doc accuracy**: fixed stale counts everywhere (see table), the
  `shear-rotation` IoU inconsistency (0.972→0.988), and the false README /
  `CONFORMANCE.md` claim that `\t` animates clip/position (the
  transformability matrix in `apply_transform_tags` ignores them).
- **Robustness re-audit**: production code has no `unwrap`/`expect`/
  `panic!`/`todo!`/`unsafe` except one guard-adjacent `expect` in
  `smart_wrap_lines` (safe by construction); all 171 pedantic cast
  sites triaged — one real bug found and fixed (`blend_pixel` color
  channels could wrap mod 256 for tiny `out_a`, e.g. 763/2=381 →
  125 instead of 255; now saturates like `blend_pixel_premul`,
  `test_blend_pixel_saturates_tiny_alpha`). Everything else verified:
  bounds-checked indices, clamped casts, checked allocation math,
  capped drawing/blur/gap budgets, wasm32-safe `usize` conversions.

## Verification commands and results

| Command | Result |
|---|---|
| `cargo test --locked --all-features` | 368 passed, 0 failed (352 lib + 2 corpus + 1 golden + 2 reference + 11 robustness) |
| `cargo test --locked --no-default-features` | 368 passed, 0 failed (same breakdown as above) |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `cargo check --target wasm32-unknown-unknown --tests --locked` | ok |
| `cargo test --test reference -- --nocapture` | 48/48 gated pass, 0 pending, 3 known-divergent (see report) |
| `cargo test --test golden` | 51/51 byte-exact pass |
| `wasm-pack build --target web --out-dir pkg` | ok |
| `bun install --frozen-lockfile` + `bun run typecheck` | clean |
| `bun test` | 41 passed, 0 failed (13 demo/paths + 17 server HTTP + 11 worker-backend) |
| `wasm-pack test --headless --chrome` | runs in CI (browser); `cargo check` covers the wasm target locally |
| fuzz smoke (`parse_ass`, `drawing`, `render`) | CI-gated on Linux (`fuzz-smoke` job); cannot link locally (Windows MSVC `LNK2001`, no C toolchain in WSL). Stable-toolchain hostile-value coverage passes locally: `tests/robustness.rs` (11) + `tests/corpus.rs` (2) |

`cargo audit`: run it in CI; if it reports the known unmaintained-`ttf-parser`
advisory (transitive via `ab_glyph`), that is pre-existing and informational —
this crate parses font metadata itself and does not depend on `ttf-parser`
directly.

Note: CI status for a given HEAD cannot be confirmed from local data alone;
the rows above are local runs. The listed GitHub Actions jobs exist in
`.github/workflows/ci.yml` and gate the same commands.

## Intentional compatibility differences

(See `CONFORMANCE.md` for the test-backed matrix.)

1. Blur runs before clipping so blurred pixels cannot bleed outside the clip
   region.
2. `\r` preserves line-global tags (position, move, origin, clips,
   fades) plus alignment; everything else, including drawing mode, resets.
3. Rotation uses libass's perspective distance (312.5 × vertical
   resolution ratio); extreme angles degrade to empty glyphs instead of
   over-allocating. (`frx`/`fry` inverse-map and `\fay` per-run shear
   reset verified against libass references; all 48 gated fixtures pass.)
4. No complex shaping (LTR `ab_glyph` only: no HarfBuzz, RTL, ligatures, or
   Indic/Arabic contextual forms); no system-font lookup. Cluster-aware
   deterministic fallback covers already-loaded faces.
5. Unhinted coverage rasterizer: ~1px placement/AA differences versus
   libass/FreeType hinted outlines (measured, not gated, in reference tests).
6. Multi-line opaque boxes draw per-line boxes like libass (gated:
   `opaque-box-multiline` IoU 1.000).
7. Wrap mode 3 keeps bottom-wide greedy fill (ASS-spec intent) rather than
   libass's rebalance (libass itself marks styles 0/3 handling FIXME).
8. `\K`/`\kf` sweep edges split at one device-space vertical line like
   libass (gated incl. rotation/shear/perspective combinations).
9. `\fe` parses/stores/resets but does not remap charsets.
