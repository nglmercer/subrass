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
5. **Resolve** — Base style is merged with override tags into per-segment `ResolvedStyle`s; line-global tags (`\pos`, `\move`, `\org`, `\clip`, `\fad`) apply wherever they appear
6. **Layout** — Every segment is shaped/measured with its own style; alignment, positioning, rotation origins, and opaque boxes use these per-segment dimensions
7. **Rasterize** — Glyphs are rasterized to coverage bitmaps (per-font cache; faux bold/italic only when the face lacks the style), then sheared/rotated
8. **Effects** — Elliptical outline, offset shadow, blur, then rectangular/vector clipping
9. **Composite** — Segments are alpha-blended onto the RGBA buffer
10. **Display** — Buffer is transferred to canvas via `putImageData`, or read back as bytes in a worker

## Override Tag Support Matrix

Status key: **Supported** = parsed and rendered; **Partial** = parsed, rendered with documented limits; **Parsed** = parsed but not rendered; **—** = not recognized (kept as `Unknown`, ignored by the renderer).

| Category | Supported | Partial | Parsed |
|---|---|---|---|
| Position | `\pos`, `\move` (with/without timing), `\org` | | |
| Colors/Alpha | `\c`, `\1c`–`\4c`, `\alpha`, `\1a`–`\4a` | | |
| Font | `\fn`, `\fs`, `\fsp`, `\b`, `\i`, `\u`, `\s` | | |
| Rotation/Scale | `\fr`, `\frx`, `\fry`, `\frz`, `\fscx`, `\fscy`, `\fax`, `\fay` | Rotation uses a fixed perspective distance; shear is applied pre-rotation | |
| Border/Shadow | `\bord`, `\xbord`, `\ybord`, `\shad`, `\xshad`, `\yshad` (incl. negative), `\be`, `\blur` | | |
| Clipping | `\clip`, `\iclip` (rectangular and vector) | | |
| Drawing | `\p1`–`\pN`, `\pbo`, commands `m n l b s p c` | B-splines are subdivided (no exact curve rasterizer) | |
| Fade | `\fad`, `\fade` | `\fade` with degenerate timing saturates instead of dividing by zero | |
| Karaoke | `\k` (secondary→primary at syllable start), `\K`/`\kf` (per-glyph sweep with edge glyph split), `\ko` (outline hidden from syllable start) | Sweep is per-glyph, not sub-glyph | |
| Wrap/Breaks | `\N` (hard break), `\n` (space, or break in wrap mode 2), `\h`, `\q` | Smart wrap (mode 0) balances lines via raggedness minimization — an approximation of VSFilter | |
| Reset | `\r`, `\rStyleName` (line-global state preserved) | | |
| Animation | `\t` (accel `t^accel`, optional timing) for colors, alpha, size, scales, spacing, rotation, borders, shadows, shear, clip, position | Unsupported inner tags are ignored | |
| Alignment | `\an`, legacy `\a` (SSA numbering converted) | | |
| Script fields | `PlayResX/Y`, `WrapStyle`, `ScaledBorderAndShadow` | `LayoutResX/Y`, `YCbCr Matrix` are parsed but unused (ASS-2 draft / RGB pipeline) | |
| Attachments | | | `[Fonts]`/`[Graphics]` parsed, decoded, and exposed via `get_attachment_*`; the renderer does not auto-load them — call `load_font(name, data)` |
| Misc | | | `Effect` field (Banner/Scroll not rendered); `Kerning`, `FontSizeMultiplier`, `HardLineBreak` exist as tag types but are not produced by the parser |

Position tags use the event's alignment as their anchor: for example, `\an5\pos(960,540)` centers the text on `(960,540)`, while `\an7\pos(100,150)` places its top-left corner there. ASS colors use `&HAABBGGRR&` ordering, where alpha is **transparency** (`00` opaque, `FF` transparent) — the `Color` type documents this invariant and converts explicitly at every boundary. `\2c` is the karaoke secondary color, shown before a syllable starts; `\4c` controls the shadow/back channel. Blur is applied **before** clipping so blurred pixels cannot bleed outside the clip region.

## Known Limitations

- No complex text shaping: left-to-right `ab_glyph` shaping only (no HarfBuzz, no RTL, no ligature-aware caret mapping).
- One face per text segment; per-glyph font fallback is not implemented.
- Rotation perspective distance is fixed (500 units); extreme angles degrade to empty glyphs rather than over-allocating.
- `\r` preserves line-global state (position, clip, fades, drawing mode) by design; see `AUDIT_FIXES.md`.
- Reference (libass pixel-comparison) tests and fuzz targets are not yet wired into CI; see `AUDIT_FIXES.md` for status.

## Build

```bash
# Install wasm-pack (also done automatically by ./build.sh)
cargo install wasm-pack

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
