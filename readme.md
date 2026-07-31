# subrass

A pure Rust ASS/SSA subtitle parser and renderer, compiled to WebAssembly. No libass dependency — everything is implemented natively in Rust.

## Features

- **Pure Rust** — no C/C++ FFI, no libass. Full control over the rendering pipeline.
- **ASS/SSA parsing** — Script Info, V4+ Styles, V4 Styles (SSA), Events, override tags (~40 tag types)
- **Native rendering** — glyph rasterization via `ab_glyph`, scanline fill for vector drawing, box blur, outline, shadow, clipping
- **WebAssembly** — compiles to WASM, renders to HTML Canvas via `putImageData`
- **Font management** — TTF/OTF loading, bold/italic matching, built-in fallback (DejaVu Sans)

## Architecture

```
src/
├── lib.rs                  # Crate root, WASM start hook, version()
├── api.rs                  # WASM bindings (AssDoc, SubtitleRenderer)
├── utils.rs                # Panic hook, 3D transform math helpers
├── parser/
│   ├── mod.rs              # ASS document parser (section dispatch)
│   ├── errors.rs           # Parse error types, section headers
│   ├── script_info.rs      # [Script Info] parser
│   ├── style.rs            # [V4+ Styles] parser
│   └── event.rs            # [Events] parser
├── renderer/
│   ├── mod.rs              # Main renderer orchestrator
│   ├── font.rs             # Font loading and management (ab_glyph)
│   ├── glyph_cache.rs      # Glyph rasterization cache
│   ├── shaper.rs           # Text shaping, measurement, word-wrap
│   ├── compositor.rs       # Style resolution, positioning, rendering
│   ├── drawing.rs          # ASS vector drawing parser (m/l/b/n/c) + types
│   ├── effects.rs          # Outline, shadow, blur, clipping
│   └── buffer.rs           # RGBA pixel buffer with alpha compositing
└── types/
    ├── mod.rs              # Core type re-exports
    ├── event.rs            # ASS event types
    ├── style.rs            # ASS style types
    ├── script_info.rs      # Script info types
    ├── override_tag.rs     # Override tag enum + parser (~40 tags)
    ├── color.rs            # ASS color (&HAABBGGRR&) type
    └── time.rs             # ASS timestamp type
```

## Rendering Pipeline

1. **Parse** — ASS file is parsed into `AssDocument` with script info, styles, and events
2. **Filter** — Active events are selected for the current timestamp
3. **Sort** — Events are sorted by layer for correct compositing order
4. **Resolve** — Base style is merged with per-event override tags into a `ResolvedStyle`
5. **Shape** — Text is mapped to glyph IDs with spacing and line breaks
6. **Rasterize** — Glyphs are rasterized to alpha bitmaps (faux bold via dilation)
7. **Effects** — Outline, shadow, blur, and clipping are applied
8. **Composite** — Glyphs are alpha-blended onto the RGBA buffer
9. **Display** — Buffer is transferred to canvas via `putImageData`

## Supported Override Tags

| Category | Tags |
|---|---|
| Position | `\pos`, `\move`, `\org` |
| Colors/Alpha | `\c`, `\1c`–`\4c`, `\alpha`, `\1a`–`\4a` |
| Font | `\fn`, `\fs`, `\b`, `\i`, `\u`, `\s` |
| Transform | `\frx`, `\fry`, `\frz`, `\fscx`, `\fscy` |
| Border/Shadow | `\bord`, `\shad`, `\be`, `\blur` |
| Clipping | `\clip`, `\iclip` |
| Drawing | `\p`, `\p1` vector paths |
| Fade | `\fad`, `\fade` |
| Karaoke | `\k`, `\K`/`\kf`, `\ko` |
| Wrapping | `\N`, `\n` |

**Parsed but not yet rendered:** `\q` (wrap-style override — parsed, but automatic word-wrapping is not applied)

## Build

```bash
# Install wasm-pack
cargo install wasm-pack

# Build for WebAssembly
wasm-pack build --target web

# Output will be in pkg/
```

## Usage

Exported methods keep their Rust snake_case names (no `js_name` remapping).

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
./build.sh                  # or: wasm-pack build --target web

# 2. Serve the repo root
bun server.ts               # bundled dev server → http://localhost:3001
# …or any static server, e.g.: python -m http.server 8080
```

Then open the served `demo/` page and select a video file and an ASS subtitle file. If you load only subtitles, the demo plays them on a virtual timeline.

A comprehensive test file [`demo/sample.ass`](demo/sample.ass) exercises all major features: karaoke (hard swap, sweep, outline), V4+ and V4 (SSA) styles, transforms, movement, clipping, vector drawing, multi-layer compositing, and fade effects.

## Status

### Working
- ASS/SSA parsing (Script Info, Styles, Events)
- Override tag parsing (~40 tags)
- Font loading and management
- Glyph rasterization with faux bold
- Basic positioning (numpad alignment 1–9)
- `\pos`, `\move`, `\org`
- Outline, Shadow effects
- Fade (`\fad`, `\fade`, `\1a`–`\4a`)
- Animated overrides (`\t` with accel, timing args optional)
- Rotation (`\frz` 2D, `\frx`/`\fry` projective 3D)
- Karaoke (`\k` hard swap, `\K`/`\kf` per-glyph sweep, `\ko` outline, combined with borders/blur/italic)
- SSA v4.00 `[V4 Styles]` (format-aware column mapping, alignment conversion, `AlphaLevel`, TertiaryColour)
- `\clip` / `\iclip`
- Drawing mode (`\p1` vector paths)
- Box blur (`\be`, `\blur`)
- Bold/Italic/Underline/Strikeout
- WebAssembly bindings
- Canvas rendering

### Not Yet Implemented
- Automatic word-wrapping (wrap styles 0–3; `\q` and script-level `WrapStyle` are parsed but ignored — only explicit `\N`/`\n` breaks apply)
- HarfBuzz/OpenType complex shaping
- Web Worker architecture
- `[Fonts]` section embedding
- `[Graphics]` section
- SIMD optimizations

## License

MIT
