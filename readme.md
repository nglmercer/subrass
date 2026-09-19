# subrass

A pure Rust ASS/SSA subtitle parser and renderer, compiled to WebAssembly. No libass dependency — everything is implemented natively in Rust.

## Features

- **Pure Rust** — no C/C++ FFI, no libass. Full control over the rendering pipeline.
- **ASS/SSA parsing** — Script Info, V4+ Styles, V4 Styles (SSA), Events, `[Fonts]`/`[Graphics]` attachments, override tags (see matrix below)
- **Native rendering** — glyph rasterization via `ab_glyph`, per-segment layout, scanline fill for vector drawing, elliptical outlines, shadows, blur, rectangular and vector clipping
- **WebAssembly** — compiles to WASM, renders to HTML Canvas via `putImageData`, or headlessly in a Web Worker
- **Font management** — TTF/OTF loading with sfnt metadata detection, deterministic matching, faux bold/italic synthesis, built-in fallback (DejaVu Sans)

## Architecture

```
src/
├── lib.rs                  # Crate root, WASM start hook, version()
├── api.rs                  # WASM bindings (AssDoc, SubtitleRenderer)
├── utils.rs                # Panic hook, 3D transform math helpers
├── parser/
│   ├── mod.rs              # ASS document parser (section dispatch)
│   ├── errors.rs           # Parse error types, section headers
│   ├── attachment.rs       # [Fonts]/[Graphics] attachment parser
│   ├── script_info.rs      # [Script Info] parser
│   ├── style.rs            # [V4+ Styles]/[V4 Styles] parser
│   └── event.rs            # [Events] parser
├── renderer/
│   ├── mod.rs              # Main renderer orchestrator
│   ├── font.rs             # Font loading, sfnt metadata, matching
│   ├── glyph_cache.rs      # Glyph rasterization cache (per-font keys, LRU)
│   ├── shaper.rs           # Text shaping and measurement
│   ├── compositor.rs       # Layout, style resolution, positioning, rendering
│   ├── drawing.rs          # ASS vector drawing (m/n/l/b/s/p/c) + masks
│   ├── effects.rs          # Outline, shadow, blur, clipping, fades
│   └── buffer.rs           # RGBA pixel buffer, safe allocation, warps
└── types/
    ├── mod.rs              # Core type re-exports
    ├── attachment.rs       # Embedded attachment types
    ├── event.rs            # ASS event types
    ├── style.rs            # ASS style types
    ├── script_info.rs      # Script info types
    ├── override_tag.rs     # Override tag enum + parser
    ├── color.rs            # ASS color (&HAABBGGRR&) type
    └── time.rs             # ASS timestamp type
```

## Rendering Pipeline

1. **Parse** — ASS file is parsed into `AssDocument` (script info, styles, events, attachments)
2. **Filter** — Active *dialogue* events are selected for the current timestamp (comments never render)
3. **Sort** — Events are stable-sorted by layer (equal layers keep source order)
4. **Wrap** — Automatic word-wrapping per the effective wrap style (per-event `\q` or script `WrapStyle`)
5. **Resolve** — Base style is merged with override tags into per-segment `ResolvedStyle`s; line-global tags (`\pos`, `\move`, `\org`, `\clip`/`\iclip` incl. vector, `\fad`/`\fade`, `\an`) apply wherever they appear textually, first tag wins per libass (`EVENT_POSITIONED`, `PARSED_A`, `PARSED_FADE`, first vector clip), while `\q` lays out the whole line last-wins
6. **Layout** — Every segment is shaped/measured with its own style; alignment, positioning, rotation origins, and opaque boxes use these per-segment dimensions
7. **Rasterize** — Glyphs are rasterized to coverage bitmaps (per-font cache; faux bold/italic only when the face lacks the style), then sheared/rotated
8. **Effects** — Elliptical outline, offset shadow, blur, then rectangular/vector clipping
9. **Composite** — Segments are alpha-blended onto the RGBA buffer
10. **Display** — Buffer is transferred to canvas via `putImageData`, or read back as bytes in a worker

## Override Tag Support Matrix

Status key: **Supported** = parsed and rendered; **Partial** = parsed, rendered with documented limits; **Parsed** = parsed but not rendered; **—** = not recognized (kept as `Unknown`, ignored by the renderer).

| Category | Supported | Partial | Parsed |
|---|---|---|---|
| Position | `\pos`, `\move` (with/without timing), `\org` (first positioning tag wins, first `\org` wins, like libass) | | |
| Colors/Alpha | `\c`, `\1c`–`\4c`, `\alpha`, `\1a`–`\4a` | | |
| Font | `\fn`, `\fs` (absolute, relative `\fs+N/-N`, bare reset), `\fsp`, `\b`, `\i`, `\u`, `\s` | | |
| Rotation/Scale | `\fr`, `\frx`, `\fry`, `\frz` (counterclockwise on screen), `\fscx`, `\fscy`, `\fax`, `\fay` (pre-rotation shear + `\fay` baseline slant, libass order) | Rotation uses a fixed perspective distance (see known limitations) | |
| Border/Shadow | `\bord`, `\xbord`, `\ybord`, `\shad`, `\xshad`, `\yshad` (incl. negative), `\be`, `\blur` | | |
| Clipping | `\clip`, `\iclip` (rectangular and vector; rect and vector are separate state like libass: later rect replaces + flips mode, first vector retained, both render) | | |
| Drawing | `\p1`–`\pN`, `\pbo`, commands `m n l b s p c` | B-splines are subdivided (no exact curve rasterizer) | |
| Fade | `\fad`, `\fade` (first fade tag wins, like libass) | `\fade` with degenerate timing saturates instead of dividing by zero | |
| Karaoke | `\k`, `\kt` (explicit syllable starts), `\K`/`\kf` (continuous sweep, split within glyph bitmaps), `\ko` (secondary fill + outline suppressed before start; primary + outline from start) | | |
| Wrap/Breaks | `\N` (hard break), `\n` (space, or break in wrap mode 2), `\h`, `\q`; mode 0 smart wrap (greedy fill + pairwise rebalance, libass algorithm); each line aligns independently; conservative CJK break opportunities | | |
| Reset | `\r`, `\rStyleName` (line-global state preserved) | | |
| Animation | `\t` (accel `t^accel`, optional timing) for colors, alpha, size, scales, spacing, rotation, borders, shadows, shear | Position/clip/fade/alignment/drawing/karaoke tags and nested `\t` are ignored inside `\t` | |
| Alignment | `\an`, legacy `\a` (SSA numbering converted; first tag wins, like libass) | | |
| Script fields | `PlayResX/Y`, `WrapStyle`, `ScaledBorderAndShadow` | `LayoutResX/Y`, `YCbCr Matrix` are parsed but unused (ASS-2 draft / RGB pipeline) | |
| Attachments | `[Fonts]` parsed, decoded, and best-effort auto-loaded as fallback faces (failures surface via `warnings()`); `[Graphics]` parsed, decoded, exposed via `get_attachment_*`; manual `load_font(name, data)` | | |
| Misc | `Effect` field: `Banner`, `Scroll up`, `Scroll down` (timing, band clip, edge fadeaway per VSFilter); `\fe` parsed/stored/reset (no charset remapping) | Unknown effect names render as plain events | `HardLineBreak` exists as a tag variant but is never produced (breaks are `\n` text) |

Position tags use the event's alignment as their anchor: for example, `\an5\pos(960,540)` centers the text on `(960,540)`, while `\an7\pos(100,150)` places its top-left corner there. ASS colors use `&HAABBGGRR&` ordering, where alpha is **transparency** (`00` opaque, `FF` transparent) — the `Color` type documents this invariant and converts explicitly at every boundary. `\2c` is the karaoke secondary color, shown before a syllable starts; `\4c` controls the shadow/back channel. Blur is applied **before** clipping so blurred pixels cannot bleed outside the clip region.

## Known Limitations

- No complex text shaping: left-to-right `ab_glyph` shaping only (no HarfBuzz, no RTL, no ligature-aware caret mapping).
- Per-glyph font fallback covers loaded faces in deterministic order (requested face → family alternates → other faces → built-in); no system-font lookup.
- Font collections (`.ttc`/`.otc`) are rejected; load single-face `.ttf`/`.otf` files instead.
- Rotation perspective distance follows libass (312.5 × vertical resolution ratio); extreme angles degrade to empty glyphs rather than over-allocating.
- `\r` preserves line-global state (position, move, origin, clips, fades) plus alignment, like libass; drawing mode, fonts, colors, rotation, and karaoke reset (note: libass keeps drawing mode across `\r` too — kept VSFilter-compatible here, see `CONFORMANCE.md`).
- Rasterizer differences remain by design: unhinted `ab_glyph` coverage vs libass/FreeType hinted outlines (~1px placement/AA differences; see `CONFORMANCE.md`). Fuzz targets (`fuzz/`) need nightly `cargo-fuzz`; short smoke runs are CI-gated, longer sessions run locally.

## Build

```bash
# Install wasm-pack (also done automatically by ./build.sh; pinned to 0.15.0 everywhere)
cargo install wasm-pack --version 0.15.0 --locked

# Build for WebAssembly (output in pkg/)
wasm-pack build --target web --out-dir pkg
```

## Usage

Exported methods keep their Rust snake_case names. Methods that take timestamps or sizes validate their inputs and throw on invalid values (non-finite, negative, zero, or unreasonably large); serialization of document queries also throws instead of returning defaults.

```html
<script type="module">
  import init, { SubtitleRenderer } from './pkg/subrass.js';

  await init();

  const response = await fetch('subtitles.ass');
  const assContent = await response.text();

  const renderer = new SubtitleRenderer(assContent);
  renderer.set_canvas(document.getElementById('canvas'));
  renderer.load_font('MyFont', fontBytes);   // Uint8Array of a TTF/OTF
  renderer.set_video_size(1920, 1080);       // optional, scales output

  // Render at 10 seconds (time is in milliseconds)
  renderer.render_frame(10000);
</script>
```

## Demo

The demo loads the built WASM module from `pkg/` and must be served over HTTP (ES module imports and `fetch` don't work from `file://`):

```bash
# 1. Build the WASM package (output in pkg/)
./build.sh                  # or: wasm-pack build --target web --out-dir pkg

# 2. Serve the repo root
bun run start               # bundled dev server → http://localhost:8001
```

Then open `/` (`demo/` landing page, `/basic` for main-thread rendering, `/worker` for worker rendering) and select a video file and an ASS subtitle file. If you load only subtitles, the demo plays them on a virtual timeline. Append `?debug` to enable demo logging.

A comprehensive test file [`demo/sample.ass`](demo/sample.ass) exercises all major features: karaoke (hard swap, sweep, outline), V4+ and V4 (SSA) styles, transforms, movement, clipping, vector drawing, multi-layer compositing, and fade effects.

## Development

```bash
cargo test                    # Rust unit + integration tests (native)
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
bun install && bun run typecheck   # demo + server typecheck (needs pkg/ built)
wasm-pack test --headless --chrome # browser WASM tests
```

CI runs fmt, clippy, native tests, the wasm32 compile check, browser tests, `cargo audit`, and the demo typecheck.

## Status

See [AUDIT_FIXES.md](AUDIT_FIXES.md) for the full remediation log: every fixed issue, the files changed, the tests added, and the compatibility differences that intentionally remain.

## License

MIT
