# CONFORMANCE.md — ASS compatibility record

Test-backed compatibility statement for subrass. Every "supported" claim below
names the test that proves it; `code + tests + libass output` are authoritative,
not prose. Regenerate the numbers with `cargo test --test reference -- --nocapture`.

## Reference environment (pinned)

| Item | Value |
|---|---|
| Reference renderer | libass via ffmpeg `ass` filter |
| ffmpeg version | `ffmpeg version 9.0.1-full_build-www.gyan.dev` (see `tests/reference/provenance.json` for buildconf hash; byte-identical output to the previous essentials build on all carried frames) |
| Reference font | `fonts/DejaVuSans.ttf`, sha256 `7da195a7…e9ed848954` (full hash in provenance.json) |
| Fixture video | 256x144, PlayRes 384x216, single frame per fixture at manifest time |
| Generator | `tests/reference/gen_references.ps1` (never runs during `cargo test`) |
| Subrass font bytes | identical file, embedded via `include_bytes!` (`src/renderer/font.rs`) |

## Comparison method (`tests/reference.rs`)

Different rasterizers never match byte-exact, so the harness compares structure:

- **Ink masks**: ours keys on alpha > 0 (a black outline/shadow/opaque box is
  real ink even with zero RGB); the opaque reference frame keys on raw RGB > 0.
- **Range normalization**: every stored reference frame spans 16..235 (opaque
  black renders as 16, opaque white as 235 — the ffmpeg filter chain blends in
  limited-range YUV). The harness expands reference channels with
  `(v - 16) * 255 / 219` before comparing intensities.
- **Intensity on 4x4 block averages**: absorbs ~1px placement/AA differences
  between unhinted `ab_glyph` coverage and hinted FreeType outlines while
  color/alpha/coverage bugs still fail.

Gates: bbox IoU ≥ 0.70, ink-count ratio within [0.5, 2.0], block mean error
≤ 25.0, block hard-fraction (avg error ≥ 48) ≤ 0.15. Golden image tests
(`tests/golden.rs`, byte-exact self-comparison) provide exact regression
detection on top.

## Latest reference results (all 67 gated fixtures pass, 0 pending)

| Fixture | IoU | Ink ratio | Mean | Hard |
|---|---|---|---|---|
| a-an | 0.950 | 0.95 | 11.25 | 0.0099 |
| alignment | 0.949 | 0.93 | 8.87 | 0.0000 |
| alignment-bare-first | 0.878 | 0.91 | 11.17 | 0.0000 |
| a-malformed-first | 0.878 | 0.91 | 11.17 | 0.0000 |
| an-a | 0.948 | 0.95 | 8.80 | 0.0000 |
| an-an | 0.937 | 0.93 | 9.59 | 0.0000 |
| an-malformed-first | 0.878 | 0.91 | 11.17 | 0.0000 |
| border-shadow | 0.891 | 1.07 | 18.94 | 0.1100 |
| clip | 0.814 | 1.55 | 21.80 | 0.1200 |
| deco | 0.940 | 0.86 | 15.65 | 0.0208 |
| deco-karaoke | 0.788 | 0.87 | 18.87 | 0.0000 |
| deco-rotated | 0.964 | 0.96 | 12.40 | 0.0000 |
| drawing | 0.828 | 0.83 | 11.94 | 0.0826 |
| fad-2args | 0.945 | 0.91 | 6.23 | 0.0000 |
| fad-7args | 0.945 | 0.91 | 6.23 | 0.0000 |
| fade | 0.945 | 0.90 | 8.62 | 0.0000 |
| fade-2args | 0.945 | 0.91 | 6.23 | 0.0000 |
| fade-7args | 0.945 | 0.91 | 6.23 | 0.0000 |
| fade-fad | 0.939 | 0.94 | 3.22 | 0.0000 |
| fade-invalid-arity | 0.926 | 0.91 | 12.39 | 0.0000 |
| fad-fade | 0.929 | 0.90 | 8.67 | 0.0000 |
| karaoke | 0.939 | 0.94 | 12.04 | 0.0280 |
| karaoke-early | 0.939 | 0.94 | 10.90 | 0.0132 |
| karaoke-kf | 0.918 | 0.93 | 12.25 | 0.0000 |
| karaoke-kf-combined | 0.933 | 1.03 | 12.99 | 0.0632 |
| karaoke-kf-early | 0.918 | 0.93 | 12.45 | 0.0000 |
| karaoke-kf-fax | 0.958 | 0.97 | 10.33 | 0.0000 |
| karaoke-kf-fay | 0.933 | 1.00 | 7.44 | 0.0000 |
| karaoke-kf-frx | 0.928 | 0.97 | 8.61 | 0.0000 |
| karaoke-kf-fry | 1.000 | 1.00 | 11.43 | 0.0109 |
| karaoke-kf-frz | 0.963 | 1.02 | 8.40 | 0.0100 |
| karaoke-kf-late | 0.918 | 0.93 | 12.77 | 0.0093 |
| karaoke-ko | 0.925 | 0.88 | 12.46 | 0.0000 |
| karaoke-late | 0.939 | 0.94 | 11.59 | 0.0227 |
| layers | 0.913 | 0.92 | 8.34 | 0.0091 |
| mixed-sizes | 0.958 | 0.91 | 11.03 | 0.0000 |
| move | 0.947 | 0.93 | 7.27 | 0.0000 |
| move-before-pos | 0.877 | 0.94 | 9.52 | 0.0000 |
| move-equal-times | 0.974 | 0.92 | 15.06 | 0.0370 |
| move-extra-args | 0.946 | 0.90 | 15.99 | 0.0122 |
| move-move | 0.897 | 0.96 | 7.41 | 0.0000 |
| move-reversed-times | 0.947 | 0.93 | 7.27 | 0.0000 |
| opaque-box | 0.992 | 0.99 | 8.80 | 0.0000 |
| opaque-box-multiline | 1.000 | 1.00 | 6.59 | 0.0000 |
| org-extra-args | 0.965 | 1.06 | 10.08 | 0.0000 |
| org-org | 0.967 | 1.05 | 9.91 | 0.0000 |
| plain | 0.951 | 0.90 | 10.64 | 0.0000 |
| pos-before-move | 0.940 | 0.93 | 7.43 | 0.0000 |
| pos-extra-args | 0.946 | 0.90 | 15.99 | 0.0122 |
| position | 0.941 | 0.90 | 9.11 | 0.0000 |
| pos-pos | 0.924 | 0.90 | 12.70 | 0.0135 |
| rect-vector-clip | 0.849 | 0.89 | 11.35 | 0.0000 |
| relative-fs | 0.978 | 0.87 | 11.61 | 0.0000 |
| relative-fs-early | 0.972 | 0.87 | 11.06 | 0.0000 |
| relative-fs-late | 0.972 | 0.91 | 13.86 | 0.0053 |
| reset | 0.941 | 0.93 | 13.28 | 0.0000 |
| reset-drawing | 0.889 | 0.85 | 16.43 | 0.0870 |
| rotation | 0.965 | 1.06 | 10.08 | 0.0000 |
| shear | 0.972 | 1.01 | 10.85 | 0.0000 |
| shear-rotation | 0.988 | 1.04 | 13.06 | 0.0412 |
| transform | 0.879 | 1.34 | 13.61 | 0.0226 |
| vector-clip | 0.820 | 0.91 | 7.43 | 0.0000 |
| vector-rect-clip | 0.851 | 0.91 | 9.94 | 0.0339 |
| vector-vector-clip | 0.928 | 0.99 | 7.42 | 0.0000 |
| wrap | 0.985 | 0.92 | 10.67 | 0.0000 |
| wrap-combining | 0.978 | 0.97 | 9.37 | 0.0000 |
| wrap-nbsp | 0.946 | 0.92 | 15.76 | 0.0253 |

Known-divergent (measured, not gated): `effect-banner`, `effect-scroll`
(libass ignores legacy effects and renders static text), `font-fallback`
(fontconfig fallback is environment-dependent), `wrap-cjk`,
`wrap-cjk-punct`, `wrap-mixed` (CJK breaking: default libass builds without
unibreak wrap at ASCII spaces only and overflow), `wrap-zwsp` (default
libass never breaks at U+200B and overflows).

No pending libass frames: every manifest fixture has a generated frame
(`tests/reference/gen_references.ps1`). If a future fixture lacks one,
the harness reports it as pending (never as a pass) in normal mode, and
`SUBRASS_STRICT_REFERENCES=1` (used by CI) fails the gate instead.

## Feature matrix

`P` = parser unit test, `R` = renderer/lib unit test, `G` = golden fixture,
`L` = libass reference fixture above.

| Feature | P | R | G/L | Status |
|---|---|---|---|---|
| `\pos`, `\move` (+timing), `\org` (first positioning tag wins, libass `EVENT_POSITIONED`; first `\org` wins) | ✓ | `test_shared_slots_first_tag_wins`, `test_pos_move_first_wins_both_orders`, `test_org_first_wins` | position/move/pos-before-move/move-before-pos/pos-pos/move-move/org-org G+L | Supported |
| `\c`, `\1c`–`\4c`, `\alpha`, `\1a`–`\4a` | ✓ | ✓ | reset/transform G+L | Supported |
| `\fn`, `\fs` (absolute, relative `\fs+N/-N`, bare reset, `\t` progress), `\fsp`, `\b` (numeric weights), `\i` | ✓ | `test_relative_fs_*`, `test_fs_relative_parsing_matches_libass` | mixed-sizes/relative-fs G+L | Supported |
| `\u`, `\s` (per-glyph bars from `post`/OS/2 metrics; shear/rotate/sweep with the glyph; both bars at once; spaces spanned) | ✓ | `test_deco_*`, `test_metadata_decoration_metrics` | deco/deco-rotated/deco-karaoke G+L | Supported |
| `\b` weight selection incl. faux-only-if-needed | ✓ | ✓ | — | Supported |
| `\fr`, `\frx`, `\fry`, `\frz` (CCW+, libass order/signs) | ✓ | `test_frz_positive_runs_counterclockwise` | rotation/transform G+L | Supported |
| `\fscx`, `\fscy` | ✓ | ✓ | mixed-sizes/transform G+L | Supported |
| `\fax`, `\fay` pre-rotation shear + `\fay` baseline slant (per-run reset) | ✓ | `test_shear_applies_to_rotated_text_at_render`, `test_fay_baseline_shear_*` | shear G+L (IoU 0.972), shear-rotation G+L (IoU 0.988) | Supported |
| `\bord`, `\xbord`, `\ybord`, `\shad`, `\xshad`, `\yshad`, `\be`, `\blur` | ✓ | `test_scaled_*` | border-shadow G+L | Supported |
| `ScaledBorderAndShadow` yes/no | ✓ | `test_scaled_*` | — | Supported |
| `\clip`, `\iclip` rect + vector (separate state: later rect replaces + flips mode, first vector retained, both render) | ✓ | `test_clip_libass_rect_vector_semantics` | clip/vector-clip/rect-vector-clip/vector-rect-clip/vector-vector-clip G+L | Supported |
| Drawings `\pN`, `\pbo`, `m n l b s p c` (bbox min preserved: advance = width, ink at pen + min; `\kf` splits at ink-left + frac × advance) | ✓ | `test_drawing_preserves_min_*`, `test_pbo_shifts_drawing`, `test_kf_drawing_split_at_fractional_scale` | drawing G+L (IoU 0.828), reset-drawing G+L (IoU 0.889) | Supported (B-splines are subdivided; `\pbo` uses libass asc/desc line metrics) |
| `\fad`, `\fade` (first fade tag wins, libass `PARSED_FADE`) | ✓ | `test_fad_fade_first_wins_both_orders` | fade/fad-fade/fade-fad G+L | Supported |
| `\k`, `\kt`, `\K`/`\kf` within-glyph sweep, `\ko` | ✓ | `test_kf_sweep_splits_within_glyph`, `test_karaoke_outline_suppressed_before_start` | karaoke* G+L incl. kf-{frx,fry,frz,fax,fay,combined} | Supported (device-space vertical split like libass) |
| `\N`, `\n`, `\h`, `\q`; wrap styles 0/1/2, 3≡0 (libass `wrap_style != 1` rebalance); CJK + U+3000 + U+200B breaks, open/close/small-kana glue, NBSP/ZWJ/ZWNJ glue, combining-mark glue (common scripts), currency-digit glue | ✓ | `test_wrap_style_3_matches_style_0`, `test_wrap_cjk_*`, `test_wrap_zwsp_breaks`, `test_wrap_ideographic_space_breaks`, `test_combining_marks_glue_common_scripts`, … | wrap/wrap-combining/wrap-nbsp G+L (IoU 0.985/0.978/0.946); wrap-cjk/wrap-cjk-punct/wrap-zwsp/wrap-mixed G + measured-divergent L | Supported (CJK/ZWSP diverge from default libass builds — see below) |
| Per-line alignment (1–9) | — | `test_multiline_centers_each_line`, `*_right_aligns_*`, `*_left_aligns_*` | wrap/alignment G+L | Supported |
| `\r`, `\rStyleName` (line-global + alignment + drawing mode + `\pbo` preserved, karaoke timing survives; a style-changing `\r` still splits karaoke runs) | ✓ | `test_reset_keeps_*`, `test_karaoke_runs_noop_reset_joins_run` | reset/reset-drawing G+L | Supported |
| `\t` animation (continuous style fields plus discrete/event-global tags, rectangular clips, and bounded nested transforms) | ✓ | `test_transform_animates_supported_set`, `test_transform_applies_libass_discrete_and_global_tags` | transform G+L | Supported; vector clip geometry remains discrete, matching libass |
| `\an`, legacy `\a` (first tag wins, libass `PARSED_A`; `\a4`/`\a8` quirk; bare/out-of-range resets to style) | ✓ | `test_alignment_first_tag_applies_event_wide`, `test_parse_legacy_a_quirk_and_range` | alignment/an-an/a-an/an-a G+L | Supported |
| `BorderStyle=3` opaque box (Outline colour, outline padding, per-line) | — | opaque-box tests | opaque-box G+L (IoU 0.992), opaque-box-multiline G+L (IoU 1.000) | Supported |
| `[Fonts]`/`[Graphics]` attachments (validated alphabet, section-aware headers) | ✓ | ✓ | — | Supported; fonts auto-loaded best-effort |
| `\fe` (parsed/stored/reset, no charset remap; render-neutral by test) | ✓ | `test_fe_resolve_and_reset`, `test_fe_encoding_is_render_neutral` | — | Partial |
| Legacy `Banner`, `Scroll up/down` effects | ✓ | ✓ | effect-* G (L: known-divergent) | Supported (VSFilter semantics; libass ignores) |
| Complex shaping (RTL, ligatures, Indic/Arabic) | — | — | — | Unsupported (LTR `ab_glyph` only; roadmap below) |
| `.ttc`/`.otc` collections | — | `test_font_collections_rejected` | — | Rejected with message (by design) |

## Known divergences (intentional)

1. Perspective distance follows libass exactly (312.5 × vertical
   resolution ratio); degenerate projections skip the glyph instead of
   over-allocating.
2. Unhinted coverage rasterizer: ~1px placement/AA differences vs hinted
   FreeType (visible in block mean errors 3–22, not gated tightly).
3. Wrap mode 3 rebalances like mode 0 (libass parity); the ASS spec's
   bottom-wide greedy intent is not implemented, matching libass's
   own FIXME on styles 0/3 (`test_wrap_style3_rebalances_like_0`).
4. Blur runs before clipping, matching libass's blur-then-clip order
   (libass blurs combined-bitmap runs, subrass the whole event buffer,
   so minor accumulation differences remain at glyph overlaps; inside
   gate thresholds: border-shadow IoU 0.891).
5. `\fe` does not remap charsets (parses, stores, resets, render-neutral;
   probe-grounded: `\fe129` matches default rendering both here and in
   libass, but non-Unicode byte remapping is not implemented). Decorations
   now match libass: per-glyph underline/strikeout bars follow shear and
   rotation, keep primary color under `\kf` swipes, and use post/OS/2
   metrics; only minor AA edge differences remain.
6. No system-font discovery (by design for WASM determinism): only the
   built-in fallback, `[Fonts]` auto-loads, and explicit `load_font`
   faces participate, in deterministic load order.
7. Legacy `Banner`/`Scroll` render with VSFilter timing while libass
   ignores them (measured as known-divergent, never gated).
8. CJK and U+200B line breaking: default libass builds (no unibreak)
   implement `ALLOWBREAK` as `glyph == ' '` — ASCII spaces only — and
   overflow everything else, while this renderer breaks CJK runs (with
   open/close/small-kana/NBSP/combining/currency glue), U+3000, and U+200B
   per UAX #14. This matches VSFilter behavior and libass builds *with*
   unibreak; fixtures `wrap-cjk`, `wrap-cjk-punct`, `wrap-zwsp`,
   `wrap-mixed` are `KNOWN_DIVERGENT` (probes confirm libass overflows).
9. `\pbo` uses libass drawing ascent/descent metrics (`asc = height - pbo`,
   `desc = pbo`), so single-drawing lines keep their ink anchored while mixed
   text/drawing lines use the drawing's adjusted ascent.

## Complex-shaping roadmap (deferred, not forgotten)

Full shaping (rustybuzz + a bidi algorithm) is the largest remaining
compatibility gap and is deliberately deferred: it touches every
consumer of glyph advances (layout, wrapping, karaoke spans, caches),
and landing it safely needs its own reference-fixture pass. When it
lands, the shape must be:

- `rustybuzz` for glyph substitution/positioning (ligatures, kerning,
  Arabic/Indic contextual forms, mark attachment) plus `unicode-bidi`
  (or equivalent) for visual run order, replacing scalar `ab_glyph`
  advances everywhere they are consumed today;
- cluster-aware fallback extended from combining-mark clusters
  (`cluster_font_picks`) to whole shaped runs;
- layout, wrapping, karaoke, and rendering all consuming the same
  shaped advances (the current invariant — measurement shares
  shaping's fallback picks — must hold for shaped runs too);
- new libass fixtures for Arabic joining, mixed LTR/RTL, Indic
  reordering, ligatures, and kerned pairs, gated like the rest.

Until then the renderer stays deterministically LTR/scalar (with an
RTL warning in `warnings()`) rather than shipping half-shaped text.
