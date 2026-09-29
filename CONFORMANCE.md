# ASS compatibility record

This is a test-backed compatibility record, not a claim of full ASS/libass
parity. Parsed, rendered, and reference-tested are different statuses. The
source audit baseline and inspected HEAD were both
`7c9215eef69e46ea49db1ea74da9fe5762c0dfe8`.

## Reference target

New differential tests use **libass 0.17.5**, source tag commit
`4a05d8127f525943ebf45fdc6497c9e665947f0d`, through its native `ASS_Image`
API. The generator refuses a different libass API version. Its configuration
is complex shaping, no hinting, no margins, storage size equal to output frame
size, requested pixel aspect 1, Unicode wrapping disabled, and
`ASS_FONTPROVIDER_NONE`. Only the committed DejaVu Sans bytes are loaded; no
system fallback participates. Explicit LayoutRes still triggers libass's own
aspect correction. Each sample starts with a fresh renderer.

Runtime dependencies used for the committed new frames: FreeType **2.14.3**,
HarfBuzz **14.5.0**, FriBidi **1.0.17**. The libass diagnostic reports the
FriBidi build header as **1.0.16**; that is distinct from the linked runtime.
The FreeType pkg-config/ABI version is **26.6.20**, not its release number.
libunibreak **7.0** is installed, but `ASS_FEATURE_WRAP_UNICODE` is explicitly
disabled. Fonts, input scripts, and uncompressed output hashes are recorded in
[the direct provenance](tests/compatibility/provenance.json).

The 105 historical FFmpeg fixtures remain unchanged. Their recorded product
version is `9.0.1-essentials_build-www.gyan.dev`, not the previously claimed
full build. Their libass and shaping dependency versions were not recorded;
they cannot establish the new environment's provenance. The original buildconf
and font hashes remain in [historical provenance](tests/reference/provenance.json).
The PowerShell generator now records renderer diagnostics and identifies host
font fallback; partial regeneration writes separate provenance.

## Regression gates and measured counts

Run and derive the report from actual results:

```sh
SUBRASS_STRICT_REFERENCES=1 cargo test --locked --test reference -- --nocapture > target/reference.log 2>&1
python tests/reference/report.py target/reference.log > tests/reference/results.md
```

[The generated report](tests/reference/results.md) contains per-frame metrics
and separate gated, divergent, host-converted, pending, and failure counts.
Exemptions and host-converted frames never count as gated passes. Missing
provenance, missing frames, malformed frame sizes, and strict-mode pending
references fail. A blank/blank direct sample can pass; unexpected blank output
fails. This suite is a finite sample of compatibility, not a parity proof.

Thresholds are unchanged: bbox IoU >= 0.70, ink ratio in [0.5, 2.0], mean channel
error on 4x4 blocks <= 25, and fraction of blocks with error >= 48 <= 0.15.
The direct oracle blends raw subtitle RGB over black, with no video color
conversion. Historical frames retain their existing range normalization.
The five `ycbcr-*` frames are host/video conversion artifacts, integrity checked
separately from direct raw RGBA/color-space tests.

Banner and Scroll are implemented by libass (`ass_parse.c::ass_apply_transition_effects`),
and the old claim that libass ignores them was incorrect. Both historical
effect fixtures now gate. New samples gate both Banner directions, Scroll
up/down, multiple times, and zero/small delay quantization. Timing delay is
quantized in layout/storage coordinates before display scaling.

Only `tests/golden/border-shadow.rgba` was updated: independent historical
reference IoU improved from 0.891 to 0.953 and mean error from 18.94 to 11.11 after replacing event-wide color blur with coverage-mask filtering.
Its ASS input and libass reference were not changed. Every other self-golden
and every historical reference remained unchanged; thresholds were not lowered.
New direct references are losslessly encoded as `(u32 count LE, RGBA pixel)`
runs in `.rle` files to keep the repository small.

To deliberately reproduce **new** references in the pinned environment:

```sh
python tests/compatibility/libass.py --generate
```

This command does not touch historical references or self-goldens. It records
configuration, input/output hashes, fonts, and actual linked dependencies.

## Feature status

| Feature | Parsed | Rendered | Reference-tested | Status / remaining scope |
|---|---|---|---|---|
| Collision placement | Event styles/tags | Measure bounded geometry, place, then remeasure/paint one event at a time; source order within layers; bottom moves up, top/middle down | `collision-*` at two resolutions; exact seeking assertions | Partial: frame-derived placement; libass retains prior placement during playback (see below) |
| Collision exclusions | Position/move/origin, animation, Effect | Positioned/origin/animated/scroll events neither displace nor are displaced; comments and empty bounds excluded | `collision-exclusions`, `collision-layer`, semantic color bounds | Tested for represented cases; not all pathological overflow/layout cases |
| Drawing `\fscx/y`, `\fax/y`, `\frx/y/z`, `\org`, `\t` | Yes | Independent axes; shared rotation/shear/perspective rules with text; bounded projected polygon coverage | `drawing-{scale,shear,frx,fry,frz,combined,animated}` including exact animation boundaries and 2x output | Partial: subdivision/scanline coverage; degenerate projections skip; text still transforms raster bitmaps |
| Drawing advance, `\p`, `\pbo`, mixed text, clipping, karaoke | Yes | Scaled advance/ascent; drawing bearings preserved; device-column karaoke split; `\ko` outline suppression | Historical drawing/pbo/reset/clip fixtures; `drawing-mixed`, `drawing-karaoke` | Tested cases, not exact curved-path rasterization |
| `\be` | Yes | Independent integer state, clamp 127, +0.5/truncate interpolation, 3x3 binomial kernel with repeated-pass 6-bit normalization | `blur-be`, `blur-be3`, `blur-together`, `blur-animated`; exact impulse/state tests | Reference-tested; bounded mask/run fallback for enormous runs |
| `\blur` | Yes | Independent continuous state, Gaussian coverage convolution, mask padding, outline/fill/shadow filtering before color and clipping | `blur-*`, historical border-shadow/clip tests | Partial: approximate libass Gaussian quantization and stroker coverage; no event-wide blur |
| Inline effects and resets | Yes | Compatible text glyphs and karaoke sweeps combine per style run; drawing runs stay separate; `\r`, bare `\be`, bare `\blur` reset independently | `blur-inline`, `blur-text`, state assertions and browser tests | Partial: opaque box effects still use the existing box painter |
| `\N`, applicable `\n`, `\h`, escaped braces | Yes | Leading/repeated hard breaks retained; soft breaks depend on wrap style; braces unescaped before shaping | Exact parser tests including dangling backslashes; historical wrapping gates | Parser-exact represented cases; unclosed groups consume the tail; encoding scanners always advance |
| Missing PlayRes | Presence recorded independently | 384x288 if both absent; 4:3 inference, 1280/1024 special case, integer rounding like libass | Exact parser cases; `missing-playres` differential | Rendered and reference-tested; explicit invalid dimensions remain parse errors |
| UTF-8, UTF-16LE/BE BOM, legacy bytes | Yes | BOM decoding precedes legacy event decoding; Unicode preserved | Exact BOM/astral/malformed tests, legacy-byte reference tests, browser UTF-16 test | UTF-16 malformed units/surrogates reject deterministically |
| Johab / Encoding 130 | Recognized | No decoder; byte-like values become U+FFFD | Existing charset tests plus tag/style diagnostics | **Unsupported**, with diagnostics; no passthrough claim |
| Fonts, GSUB/GPOS, bidi, collections | Yes | `harfrust`, cluster fallback, explicit loaded fonts | Historical Arabic/Hebrew/Indic/bidi/ligature/collection gates | Partial: deterministic fallback differs from host fontconfig |
| Borders/shadows, clips, fades, alignment, reset, transforms, karaoke | Yes | Existing implementation, with blur/drawing fixes above | Historical fixture gates and unit tests | Reference-tested represented cases; no general full-parity claim |
| Banner / Scroll | Yes | Motion, band clipping; VSFilter edge fade extension | Historical effect gates and direct motion samples | Partial: optional edge fade behavior has not been differentially gated |
| Graphics attachments | Yes | Exposed through attachment API | Parser/serialization tests | Parsed/API-accessible; not painted as embedded images |
| Unknown tags / effects | Retained/diagnosed | Ignored / plain dialogue fallback | Existing diagnostics tests | Unsupported |

## Intentional differences and remaining limitations

- Collision placement is recomputed from the active frame in source order.
  It is deterministic across playback, seeking, and cache clearing. libass's
  `fix_collisions` caches event rectangles: an event can retain an elevated
  slot after an earlier event ends, and playback can differ from a fresh seek.
  subrass currently chooses reproducible frame-derived placement. This is an
  explicit behavioral difference, not evidence of full playback parity.
- The five measured divergences are deterministic font fallback and CJK,
  mixed CJK, punctuation, and U+200B wrapping. subrass keeps its existing Unicode
  break behavior; the pinned reference disables Unicode wrapping. No exemption
  is claimed for Banner/Scroll. Host font selection remains opt-in for native
  `system-fonts`; WASM uses explicit/bundled fonts.
- Coverage is unhinted `ab_glyph` for text and pixel-center polygon scanlines
  for drawings. Stroker coverage, fractional shadow offsets, Gaussian kernel
  quantization, and overlap rounding differ from libass. Large separated blur
  runs fall back to bounded per-glyph masks instead of allocating without a cap.
  Near-singular shear uses the existing bounded fallback; projection poles skip
  geometry. Existing buffer, glyph, drawing-command, and sweep limits remain. Wide Gaussian
  masks use three variance-matched sliding box passes to keep work linear;
  small radii use direct Gaussian convolution. `blur-wide` gates the approximation.
- Perspective uses the existing PlayRes vertical ratio. The new transformation
  fixtures use matching explicit LayoutRes and PlayRes. Differing LayoutRes
  perspective scales and post-rotation aspect correction are not fully matched;
  unrotated text's explicit LayoutRes aspect correction is now tested.
- BorderStyle 3 uses the existing per-line axis-aligned box painter; its blur
  and rotations are not full libass parity. Vector clip geometry, complex
  B-splines, Unicode shaping across every override boundary, optional motion
  fade edges, and malformed input recovery are not comprehensively tested.
- Invalid known Script Info fields reject input, including explicit zero/negative
  PlayRes; libass is more permissive. UTF-16 malformed sequences reject rather
  than silently selecting a legacy charset. These are deliberate parser/error
  boundaries. UTF-32 and BOM-less UTF-16 are not decoded.

For a collision retention reproduction, use the two events from
`collision-staggered.ass`, put the earlier event first, end it at 1.50 seconds,
and compare direct oracle `render(script, 384, 216, [1000, 2000])` with a fresh
`render(script, 384, 216, [2000])`. The remaining event keeps its earlier slot
only in the playback oracle. This is not part of the gated fresh-frame counts.

Validation commands, results, unavailable checks, changed file groups, and
remaining failures are recorded in [COMPATIBILITY-FIXES.md](COMPATIBILITY-FIXES.md).
