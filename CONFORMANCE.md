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

## Latest reference results (all 35 gated fixtures pass, 0 pending)

| Fixture | IoU | Ink ratio | Mean | Hard |
|---|---|---|---|---|
| alignment | 0.949 | 0.93 | 8.87 | 0.0000 |
| border-shadow | 0.891 | 1.07 | 18.94 | 0.1100 |
| clip | 0.814 | 1.55 | 21.80 | 0.1200 |
| drawing | 0.828 | 0.83 | 11.94 | 0.0826 |
| fade | 0.945 | 0.90 | 8.77 | 0.0000 |
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
| opaque-box | 0.992 | 0.99 | 8.80 | 0.0000 |
| opaque-box-multiline | 1.000 | 1.00 | 6.59 | 0.0000 |
| plain | 0.951 | 0.90 | 10.64 | 0.0000 |
| position | 0.941 | 0.90 | 9.11 | 0.0000 |
| relative-fs | 0.978 | 0.87 | 11.61 | 0.0000 |
| relative-fs-early | 0.972 | 0.87 | 11.06 | 0.0000 |
| relative-fs-late | 0.972 | 0.91 | 13.86 | 0.0053 |
| reset | 0.941 | 0.93 | 13.28 | 0.0000 |
| rotation | 0.965 | 1.06 | 10.08 | 0.0000 |
| shear | 0.972 | 1.01 | 10.85 | 0.0000 |
| shear-rotation | 0.988 | 1.04 | 13.06 | 0.0412 |
| transform | 0.879 | 1.34 | 13.61 | 0.0226 |
| vector-clip | 0.820 | 0.91 | 7.43 | 0.0000 |
| wrap | 0.985 | 0.92 | 10.67 | 0.0000 |

Known-divergent (measured, not gated): `effect-banner`, `effect-scroll`
(libass ignores legacy effects and renders static text), `font-fallback`
(fontconfig fallback is environment-dependent).

No pending libass frames: every manifest fixture has a generated frame
(`tests/reference/gen_references.ps1`). If a future fixture lacks one,
the harness reports it as pending (never as a pass) in normal mode, and
`SUBRASS_STRICT_REFERENCES=1` (used by CI) fails the gate instead.

## Feature matrix

`P` = parser unit test, `R` = renderer/lib unit test, `G` = golden fixture,
`L` = libass reference fixture above.

| Feature | P | R | G/L | Status |
|---|---|---|---|---|
| `\pos`, `\move` (+timing), `\org` | ✓ | ✓ | position/move G+L | Supported |
| `\c`, `\1c`–`\4c`, `\alpha`, `\1a`–`\4a` | ✓ | ✓ | reset/transform G+L | Supported |
| `\fn`, `\fs` (absolute, relative `\fs+N/-N`, bare reset, `\t` progress), `\fsp`, `\b` (numeric weights), `\i`, `\u`, `\s` | ✓ | `test_relative_fs_*`, `test_fs_relative_parsing_matches_libass` | mixed-sizes/relative-fs G+L | Supported |
| `\b` weight selection incl. faux-only-if-needed | ✓ | ✓ | — | Supported |
| `\fr`, `\frx`, `\fry`, `\frz` (CCW+, libass order/signs) | ✓ | `test_frz_positive_runs_counterclockwise` | rotation/transform G+L | Supported |
| `\fscx`, `\fscy` | ✓ | ✓ | mixed-sizes/transform G+L | Supported |
| `\fax`, `\fay` pre-rotation shear + `\fay` baseline slant (per-run reset) | ✓ | `test_shear_applies_to_rotated_text_at_render`, `test_fay_baseline_shear_*` | shear G+L (IoU 0.972), shear-rotation G+L (IoU 0.972) | Supported |
| `\bord`, `\xbord`, `\ybord`, `\shad`, `\xshad`, `\yshad`, `\be`, `\blur` | ✓ | `test_scaled_*` | border-shadow G+L | Supported |
| `ScaledBorderAndShadow` yes/no | ✓ | `test_scaled_*` | — | Supported |
| `\clip`, `\iclip` rect + vector | ✓ | ✓ | clip/vector-clip G+L | Supported |
| Drawings `\pN`, `\pbo`, `m n l b s p c` | ✓ | ✓ | drawing G+L (IoU 0.828) | Supported (B-splines subdivided) |
| `\fad`, `\fade` | ✓ | ✓ | fade G+L | Supported |
| `\k`, `\kt`, `\K`/`\kf` within-glyph sweep, `\ko` | ✓ | `test_kf_sweep_splits_within_glyph`, `test_karaoke_outline_suppressed_before_start` | karaoke* G+L incl. kf-{frx,fry,frz,fax,fay,combined} | Supported (device-space vertical split like libass) |
| `\N`, `\n`, `\h`, `\q`; wrap styles 0/1/2/3; conservative CJK breaks | ✓ | `test_wrap_style_0_balances_lines`, `test_wrap_cjk_*`, … | wrap G+L (IoU 0.985) | Supported |
| Per-line alignment (1–9) | — | `test_multiline_centers_each_line`, `*_right_aligns_*`, `*_left_aligns_*` | wrap/alignment G+L | Supported |
| `\r`, `\rStyleName` (line-global preserve) | ✓ | reset tests | reset G+L | Supported |
| `\t` animation (colors/alpha/size/scales/spacing/rotation/border/shadow/shear/clip/pos) | ✓ | ✓ | transform G+L | Supported; unsupported inner tags ignored (documented) |
| `\an`, legacy `\a` | ✓ | ✓ | alignment G+L | Supported |
| `BorderStyle=3` opaque box (Outline colour, outline padding, per-line) | — | opaque-box tests | opaque-box G+L (IoU 0.992), opaque-box-multiline G+L (IoU 1.000) | Supported |
| `[Fonts]`/`[Graphics]` attachments (validated alphabet, section-aware headers) | ✓ | ✓ | — | Supported; fonts auto-loaded best-effort |
| `\fe` (parsed/stored/reset, no charset remap) | ✓ | `test_fe_resolve_and_reset` | — | Partial |
| Legacy `Banner`, `Scroll up/down` effects | ✓ | ✓ | effect-* G (L: known-divergent) | Supported (VSFilter semantics; libass ignores) |
| Complex shaping (RTL, ligatures, Indic/Arabic) | — | — | — | Unsupported (LTR `ab_glyph` only) |
| `.ttc`/`.otc` collections | — | `test_*ttcf*` rejection tests | — | Rejected with message (by design) |

## Known divergences (intentional)

1. Perspective distance follows libass exactly (312.5 × vertical
   resolution ratio); degenerate projections skip the glyph instead of
   over-allocating.
2. Unhinted coverage rasterizer: ~1px placement/AA differences vs hinted
   FreeType (visible in block mean errors 7–22, not gated tightly).
3. Wrap mode 3 keeps bottom-wide greedy fill (ASS-spec intent); libass applies
   the same rebalance as mode 0 (and marks styles 0/3 FIXME).
4. Blur runs before clipping (no bleed outside clip region).
5. `\fe` does not remap charsets; underline/strikeout do not slant with `\fay`.
