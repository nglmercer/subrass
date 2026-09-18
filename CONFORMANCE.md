# CONFORMANCE.md — ASS compatibility record

Test-backed compatibility statement for subrass. Every "supported" claim below
names the test that proves it; `code + tests + libass output` are authoritative,
not prose. Regenerate the numbers with `cargo test --test reference -- --nocapture`.

## Reference environment (pinned)

| Item | Value |
|---|---|
| Reference renderer | libass via ffmpeg `ass` filter |
| ffmpeg version | `ffmpeg version 9.0.1-essentials_build-www.gyan.dev` (see `tests/reference/provenance.json` for buildconf hash) |
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

## Latest reference results (all 20 gated fixtures pass)

| Fixture | IoU | Ink ratio | Mean | Hard |
|---|---|---|---|---|
| alignment | 0.949 | 0.93 | 8.87 | 0.0000 |
| border-shadow | 0.891 | 1.07 | 18.94 | 0.1100 |
| clip | 0.814 | 1.55 | 21.80 | 0.1200 |
| drawing | 0.828 | 0.83 | 11.94 | 0.0826 |
| fade | 0.945 | 0.90 | 8.77 | 0.0000 |
| karaoke | 0.939 | 0.94 | 12.04 | 0.0280 |
| karaoke-kf | 0.918 | 0.93 | 12.25 | 0.0000 |
| karaoke-ko | 0.925 | 0.88 | 12.46 | 0.0000 |
| layers | 0.913 | 0.92 | 8.34 | 0.0091 |
| mixed-sizes | 0.958 | 0.91 | 11.03 | 0.0000 |
| move | 0.947 | 0.93 | 7.27 | 0.0000 |
| opaque-box | 0.992 | 0.99 | 8.80 | 0.0000 |
| plain | 0.951 | 0.90 | 10.64 | 0.0000 |
| position | 0.941 | 0.90 | 9.11 | 0.0000 |
| reset | 0.941 | 0.93 | 13.28 | 0.0000 |
| rotation | 0.955 | 1.22 | 22.12 | 0.1398 |
| shear | 0.972 | 1.01 | 10.85 | 0.0000 |
| transform | 0.879 | 1.34 | 13.61 | 0.0226 |
| vector-clip | 0.820 | 0.91 | 7.43 | 0.0000 |
| wrap | 0.985 | 0.92 | 10.67 | 0.0000 |

Known-divergent (measured, not gated): `effect-banner`, `effect-scroll`
(libass ignores legacy effects and renders static text), `font-fallback`
(fontconfig fallback is environment-dependent).

## Feature matrix

`P` = parser unit test, `R` = renderer/lib unit test, `G` = golden fixture,
`L` = libass reference fixture above.

| Feature | P | R | G/L | Status |
|---|---|---|---|---|
| `\pos`, `\move` (+timing), `\org` | ✓ | ✓ | position/move G+L | Supported |
| `\c`, `\1c`–`\4c`, `\alpha`, `\1a`–`\4a` | ✓ | ✓ | reset/transform G+L | Supported |
| `\fn`, `\fs` (absolute), `\fsp`, `\b` (numeric weights), `\i`, `\u`, `\s` | ✓ | ✓ | mixed-sizes G+L | Supported |
| `\b` weight selection incl. faux-only-if-needed | ✓ | ✓ | — | Supported |
| `\fr`, `\frx`, `\fry`, `\frz` (CCW+, libass order/signs) | ✓ | `test_frz_positive_runs_counterclockwise` | rotation/transform G+L | Supported |
| `\fscx`, `\fscy` | ✓ | ✓ | mixed-sizes/transform G+L | Supported |
| `\fax`, `\fay` pre-rotation shear + `\fay` baseline slant | ✓ | `test_shear_applies_to_rotated_text_at_render`, `test_fay_baseline_shear_*` | shear G+L (IoU 0.972) | Supported |
| `\bord`, `\xbord`, `\ybord`, `\shad`, `\xshad`, `\yshad`, `\be`, `\blur` | ✓ | `test_scaled_*` | border-shadow G+L | Supported |
| `ScaledBorderAndShadow` yes/no | ✓ | `test_scaled_*` | — | Supported |
| `\clip`, `\iclip` rect + vector | ✓ | ✓ | clip/vector-clip G+L | Supported |
| Drawings `\pN`, `\pbo`, `m n l b s p c` | ✓ | ✓ | drawing G+L (IoU 0.828) | Supported (B-splines subdivided) |
| `\fad`, `\fade` | ✓ | ✓ | fade G+L | Supported |
| `\k`, `\kt`, `\K`/`\kf` within-glyph sweep, `\ko` | ✓ | `test_kf_sweep_splits_within_glyph`, `test_karaoke_outline_suppressed_before_start` | karaoke* G+L | Supported |
| `\N`, `\n`, `\h`, `\q`; wrap styles 0/1/2/3 | ✓ | `test_wrap_style_0_balances_lines`, … | wrap G+L (IoU 0.985) | Supported (spaces-only breaks, no CJK opportunities) |
| Per-line alignment (1–9) | — | `test_multiline_centers_each_line`, `*_right_aligns_*`, `*_left_aligns_*` | wrap/alignment G+L | Supported |
| `\r`, `\rStyleName` (line-global preserve) | ✓ | reset tests | reset G+L | Supported |
| `\t` animation (colors/alpha/size/scales/spacing/rotation/border/shadow/shear/clip/pos) | ✓ | ✓ | transform G+L | Supported; unsupported inner tags ignored (documented) |
| `\an`, legacy `\a` | ✓ | ✓ | alignment G+L | Supported |
| `BorderStyle=3` opaque box (Outline colour, outline padding) | — | opaque-box tests | opaque-box G+L (IoU 0.992) | Supported single-line; multi-line covers whole block (libass: per-line) |
| `[Fonts]`/`[Graphics]` attachments (validated alphabet, section-aware headers) | ✓ | ✓ | — | Supported; fonts auto-loaded best-effort |
| `\fe` (parsed/stored/reset, no charset remap) | ✓ | `test_fe_resolve_and_reset` | — | Partial |
| Legacy `Banner`, `Scroll up/down` effects | ✓ | ✓ | effect-* G (L: known-divergent) | Supported (VSFilter semantics; libass ignores) |
| Complex shaping (RTL, ligatures, Indic/Arabic) | — | — | — | Unsupported (LTR `ab_glyph` only) |
| `.ttc`/`.otc` collections | — | `test_*ttcf*` rejection tests | — | Rejected with message (by design) |

## Known divergences (intentional)

1. Fixed perspective distance (500 × vertical resolution ratio); degenerate
   projections skip the glyph instead of over-allocating.
2. Unhinted coverage rasterizer: ~1px placement/AA differences vs hinted
   FreeType (visible in block mean errors 7–22, not gated tightly).
3. Multi-line opaque box covers the whole block (libass: per-line boxes).
4. Wrap mode 3 keeps bottom-wide greedy fill (ASS-spec intent); libass applies
   the same rebalance as mode 0 (and marks styles 0/3 FIXME).
5. Blur runs before clipping (no bleed outside clip region).
6. `\fe` does not remap charsets; underline/strikeout do not slant with `\fay`.
