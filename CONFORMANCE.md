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
- **Range normalization**: legacy FFmpeg/libass fixtures without an explicit
  matrix are expanded from 16..235 with `(v - 16) * 255 / 219`. The
  `ycbcr-*` files are retained as explicitly documented host/video-conversion
  artifacts and are not evidence of raw subtitle RGB behavior. Raw YCbCr
  semantics are gated by direct `SubtitleRenderer` RGBA tests; explicit
  downstream conversion is covered by `types::color_space` tests.
- **Intensity on 4x4 block averages**: absorbs ~1px placement/AA differences
  between unhinted `ab_glyph` coverage and hinted FreeType outlines while
  color/alpha/coverage bugs still fail.

Gates: bbox IoU ≥ 0.70, ink-count ratio within [0.5, 2.0], block mean error
≤ 25.0, block hard-fraction (avg error ≥ 48) ≤ 0.15. Golden image tests
(`tests/golden.rs`, byte-exact self-comparison) provide exact regression
detection on top.

## Latest reference results (98 gated pass, 7 known-divergent measured, 0 open failures, 0 pending)

| Fixture | IoU | Ink ratio | Mean | Hard |
|---|---|---|---|---|
| a-an | 0.950 | 0.95 | 11.25 | 0.0099 |
| a-malformed-first | 0.878 | 0.91 | 11.17 | 0.0000 |
| alignment | 0.949 | 0.93 | 8.87 | 0.0000 |
| alignment-bare-first | 0.878 | 0.91 | 11.17 | 0.0000 |
| an-a | 0.948 | 0.95 | 8.80 | 0.0000 |
| an-an | 0.937 | 0.93 | 9.59 | 0.0000 |
| an-malformed-first | 0.878 | 0.91 | 11.17 | 0.0000 |
| arabic | 0.943 | 0.93 | 10.35 | 0.0104 |
| border-shadow | 0.891 | 1.07 | 18.94 | 0.1100 |
| clip | 0.975 | 1.10 | 16.14 | 0.0625 |
| deco | 0.895 | 0.85 | 15.61 | 0.0312 |
| deco-karaoke | 0.788 | 0.86 | 19.95 | 0.0152 |
| deco-rotated | 0.964 | 0.95 | 13.49 | 0.0000 |
| drawing | 0.891 | 0.89 | 4.33 | 0.0000 |
| effect-banner | 0.950 | 0.92 | 17.60 | 0.0526 |
| effect-scroll | 0.990 | 0.91 | 7.44 | 0.0000 |
| fad-2args | 0.945 | 0.91 | 6.23 | 0.0000 |
| fad-7args | 0.945 | 0.91 | 6.23 | 0.0000 |
| fad-fade | 0.929 | 0.90 | 8.67 | 0.0000 |
| fade | 0.945 | 0.90 | 8.62 | 0.0000 |
| fade-2args | 0.945 | 0.91 | 6.23 | 0.0000 |
| fade-7args | 0.945 | 0.91 | 6.23 | 0.0000 |
| fade-fad | 0.939 | 0.94 | 3.22 | 0.0000 |
| fade-invalid-arity | 0.926 | 0.91 | 12.39 | 0.0000 |
| fe-charset | 0.954 | 0.92 | 13.15 | 0.0089 |
| font-fallback | 0.856 | 0.80 | 57.80 | 0.5043 |
| hebrew | 0.900 | 0.94 | 7.85 | 0.0000 |
| font-collection | 0.986 | 1.22 | 8.16 | 0.0000 |
| indic | 0.867 | 1.33 | 7.06 | 0.0000 |
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
| kerning | 0.966 | 0.94 | 11.06 | 0.0000 |
| layers | 0.913 | 0.92 | 8.34 | 0.0091 |
| ligature | 0.947 | 0.93 | 11.13 | 0.0000 |
| mixed-bidi | 0.971 | 0.92 | 10.58 | 0.0041 |
| mixed-sizes | 0.958 | 0.91 | 11.03 | 0.0000 |
| move | 0.947 | 0.93 | 7.27 | 0.0000 |
| move-before-pos | 0.877 | 0.94 | 9.52 | 0.0000 |
| move-equal-times | 0.974 | 0.92 | 15.06 | 0.0370 |
| move-extra-args | 0.946 | 0.90 | 15.99 | 0.0122 |
| move-move | 0.897 | 0.96 | 7.41 | 0.0000 |
| move-reversed-times | 0.947 | 0.93 | 7.27 | 0.0000 |
| numeric-prefix-tags | 0.946 | 0.92 | 16.91 | 0.0091 |
| opaque-box | 0.932 | 0.99 | 9.82 | 0.0000 |
| opaque-box-multiline | 0.982 | 1.01 | 6.50 | 0.0000 |
| org-extra-args | 0.965 | 1.06 | 10.08 | 0.0000 |
| org-org | 0.967 | 1.05 | 9.91 | 0.0000 |
| pbo-mixed | 0.904 | 0.84 | 6.91 | 0.0000 |
| pbo-negative | 0.821 | 0.83 | 0.92 | 0.0000 |
| pbo-positive | 0.856 | 0.86 | 7.49 | 0.0000 |
| plain | 0.951 | 0.90 | 10.64 | 0.0000 |
| pos-before-move | 0.940 | 0.93 | 7.43 | 0.0000 |
| pos-extra-args | 0.946 | 0.90 | 15.99 | 0.0122 |
| pos-pos | 0.924 | 0.90 | 12.70 | 0.0135 |
| position | 0.941 | 0.90 | 9.11 | 0.0000 |
| rect-vector-clip | 0.861 | 0.88 | 10.51 | 0.0000 |
| relative-fs | 0.978 | 0.87 | 11.61 | 0.0000 |
| relative-fs-early | 0.972 | 0.87 | 11.06 | 0.0000 |
| relative-fs-late | 0.972 | 0.91 | 13.86 | 0.0053 |
| reset | 0.941 | 0.93 | 13.28 | 0.0000 |
| reset-drawing | 0.934 | 0.90 | 5.95 | 0.0000 |
| rotation | 0.965 | 1.06 | 10.08 | 0.0000 |
| shear | 0.972 | 1.01 | 10.85 | 0.0000 |
| shear-rotation | 0.988 | 1.04 | 13.06 | 0.0412 |
| transform | 0.879 | 1.34 | 13.61 | 0.0226 |
| transform-an | 0.924 | 0.91 | 11.62 | 0.0000 |
| transform-clip | 0.975 | 0.98 | 10.53 | 0.0000 |
| transform-discrete | 0.913 | 0.95 | 15.21 | 0.0000 |
| transform-fade | 0.945 | 0.89 | 11.86 | 0.0130 |
| transform-iclip | 0.913 | 0.92 | 11.23 | 0.0000 |
| transform-karaoke | 0.939 | 0.95 | 12.38 | 0.0000 |
| transform-nested | 0.887 | 1.00 | 9.60 | 0.0000 |
| transform-nested-accel | 0.921 | 0.93 | 7.95 | 0.0000 |
| transform-nondefault-playres | 1.000 | 0.82 | 6.48 | 0.0000 |
| transform-nondefault-playres-early | 0.990 | 0.81 | 5.65 | 0.0000 |
| transform-nondefault-playres-late | 0.967 | 0.86 | 8.11 | 0.0227 |
| transform-org | 0.967 | 1.03 | 8.05 | 0.0000 |
| transform-pos | 0.945 | 0.93 | 10.97 | 0.0286 |
| transform-q | 0.958 | 0.94 | 13.06 | 0.0042 |
| vector-clip | 0.820 | 0.91 | 7.43 | 0.0000 |
| vector-rect-clip | 0.863 | 0.89 | 8.98 | 0.0000 |
| vector-vector-clip | 0.950 | 0.97 | 7.08 | 0.0000 |
| wrap | 0.985 | 0.92 | 10.67 | 0.0000 |
| wrap-cjk | 0.381 | 1.49 | 68.50 | 0.7402 |
| wrap-cjk-punct | 0.420 | 1.57 | 57.07 | 0.5199 |
| wrap-combining | 1.000 | 0.99 | 6.31 | 0.0000 |
| wrap-mixed | 0.377 | 1.32 | 58.34 | 0.5165 |
| wrap-nbsp | 0.946 | 0.91 | 12.64 | 0.0000 |
| wrap-zwsp | 0.373 | 0.91 | 66.44 | 0.6212 |
| ycbcr-none | 0.985 | 0.94 | 2.54 | 0.0000 |
| ycbcr-pc601 | 0.985 | 0.94 | 2.54 | 0.0000 |
| ycbcr-pc709 | 0.985 | 0.94 | 2.54 | 0.0000 |
| ycbcr-tv601 | 0.985 | 0.94 | 2.50 | 0.0000 |
| ycbcr-tv709 | 0.985 | 0.94 | 2.50 | 0.0000 |

Open failures: none. `indic` loads the committed OFL Noto file in both
renderers; `transform-iclip` retains meaningful glyph area; and the
non-default-PlayRes transform has visible early/mid/late samples whose
committed libass frames are asserted to differ. The TTC fixture uses an
isolated collection-only libass fonts directory.

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
| `ScaledBorderAndShadow` yes/no (unscaled = 1:1 video px: VSFilter/legacy-libass; current libass without storage size ignores the flag) | ✓ | `test_scaled_*` | — | Supported |
| `LayoutResX/Y` (libass `ass_layout_res`: blur + unscaled-border denominators; unset = video size) | ✓ | `test_layout_res_drives_unscaled_borders_like_libass` | — | Supported |
| `Kerning:` header (default off, like libass `calloc` track); `liga`/`clig` off under non-zero `\fsp` | ✓ | `test_opentype_kerning_matches_libass_default_off`, `test_kerning_header_parses_like_libass_bool` | alignment G+L (mean_err 8.87) | Supported |
| OpenType shaping (`harfrust` GSUB/GPOS + bidi visual runs; `.ttc`/`.otc` all faces; explicit native system fonts) | ✓ | `test_opentype_shaping_uses_gsub_bidi_and_marks`, `test_system_font_discovery_is_explicit_and_idempotent` | arabic/hebrew/mixed-bidi/indic/font-collection G+L | Supported |
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
| `\fe` (legacy-byte charset bridge; Unicode text preserved; parsed/stored/reset) | Partial | `test_ass_charset_mapping_preserves_unicode_scripts`, `test_symbol_bytes_use_private_use_cmap_and_preserve_ass_breaks`, `test_fe_resolve_and_reset` | fe-charset G+L (IoU 0.954) | Windows-125x, Shift-JIS, CP949, GBK, Big5, Thai, and Symbol PUA mapping are supported; malformed bytes are deterministic. Johab codec and charset-based font linking remain unsupported |
| Legacy `Banner`, `Scroll up/down` effects | ✓ | ✓ | effect-* G (L: known-divergent) | Supported (VSFilter semantics; libass ignores) |
| Complex shaping (Arabic/Hebrew/mixed-bidi, ligatures, kerning, marks; `harfrust` GSUB/GPOS + bidi) | ✓ | `test_opentype_shaping_uses_gsub_bidi_and_marks`, `test_noto_advances_use_win_divisor` | arabic/hebrew/mixed-bidi/ligature/kerning/indic G+L | Supported |
| `.ttc`/`.otc` collections (every face: metadata, matching, fallback, shaping identity) | ✓ | `test_font_collections_load_every_face`, `test_committed_collection_matches_styles_and_preserves_face_indices` | font-collection G+L | Supported; regular/bold-italic/Indic face indices gated |
| `YCbCr Matrix` metadata and raw RGB behavior | ✓ | `test_parse_all_libass_ycbcr_matrix_values`, `raw_rgba_ignores_ycbcr_matrix_metadata_without_host_conversion` | ycbcr-* retained as host-conversion artifacts | Raw RGBA supported; explicit downstream conversion requires a supplied `VideoColorSpace` |

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
5. `\fe` has no charset-based font linking and Johab remains Unicode-neutral;
   Symbol bytes are mapped to U+F000..U+F0FF and require a loaded Symbol-compatible face;
   supported byte streams decode before shaping, invalid sequences become
   U+FFFD, and already-valid Unicode is never re-encoded.
6. No implicit system-font discovery: WASM builds stay deterministic
   (built-in fallback, `[Fonts]` auto-loads, and explicit `load_font`
   faces in load order). Native builds may opt in with the
   `system-fonts` feature, which loads each discovered family once
   after explicit faces
   (`test_system_font_discovery_is_explicit_and_idempotent`).
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

## Complex shaping (landed)

Shaping uses `harfrust` for GSUB/GPOS substitution/positioning plus
`unicode-bidi` visual runs, with cluster-aware fallback
(`cluster_font_picks`) extended to whole shaped runs. Measurement,
wrapping, layout, karaoke, and rendering all consume the same shaped
advances. Gated by `test_opentype_shaping_uses_gsub_bidi_and_marks`
and the arabic/hebrew/mixed-bidi/ligature/kerning fixtures (G+L);
`indic` renders real Devanagari through the same committed Noto bytes in
both renderers and is gated. `test_noto_advances_use_win_divisor` pins the
FreeType Win-metrics scale; `font-collection` additionally gates shaping
from the preserved TTC face index.
