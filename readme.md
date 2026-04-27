# subrass

A pure Rust ASS/SSA subtitle parser and renderer, compiled to WebAssembly. No libass dependency — everything is implemented natively in Rust.

## Features

- **Pure Rust** — no C/C++ FFI, no libass. Full control over the rendering pipeline.
- **ASS/SSA parsing** — Script Info, V4+ Styles, V4 Styles, Events, override tags (~40 tag types)
- **Native rendering** — glyph rasterization via `ab_glyph`, scanline fill for vector drawing, box blur, outline, shadow, clipping
- **WebAssembly** — compiles to WASM, renders to HTML Canvas via `putImageData`
- **Font management** — TTF/OTF loading, bold/italic matching, built-in fallback (DejaVu Sans)

## Architecture

```
src/
├── api.rs                  # WASM bindings (AssDoc, SubtitleRenderer)
├── parser/
│   ├── mod.rs              # ASS document parser
│   └── override_tag.rs     # Override tag parser (~40 tags)
├── renderer/
│   ├── mod.rs              # Main renderer orchestrator
│   ├── font.rs             # Font loading and management (ab_glyph)
│   ├── glyph_cache.rs      # Glyph rasterization with LRU cache
│   ├── shaper.rs           # Text shaping, measurement, word-wrap
│   ├── compositor.rs       # Style resolution, positioning, rendering
│   ├── drawing.rs          # ASS vector drawing parser (m/l/b/n/c)
│   ├── effects.rs          # Outline, shadow, blur, clipping
│   └── buffer.rs           # RGBA pixel buffer with alpha compositing
└── types/
    ├── mod.rs              # Core types (Event, Style, ScriptInfo)
    ├── event.rs            # ASS event types
    ├── style.rs            # ASS style types
    ├── override_tag.rs     # Override tag enum
    └── drawing.rs          # Drawing command types
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
| Wrapping | `\q`, `\N`, `\n` |

**Parsed but not yet rendered:** `\t` (animated overrides), `\frx`/`\fry` (3D rotation), karaoke (`\k`, `\K`, `\kf`, `\ko`)

## Build

```bash
# Install wasm-pack
cargo install wasm-pack

# Build for WebAssembly
wasm-pack build --target web

# Output will be in pkg/
```

## Usage

```html
<script type="module">
  import init, { SubtitleRenderer } from './pkg/subrass.js';

  await init();

  const response = await fetch('subtitles.ass');
  const assContent = await response.text();

  const renderer = new SubtitleRenderer(assContent);
  renderer.setCanvas(document.getElementById('canvas'));
  renderer.setFont('MyFont', fontBytes);

  // Render at 10 seconds
  renderer.renderFrame(10000);
</script>
```

## Demo

Open `demo/index.html` in a browser. Select a video file and an ASS subtitle file to see the renderer in action.

## Status

### Working
- ASS/SSA parsing (Script Info, Styles, Events)
- Override tag parsing (~40 tags)
- Font loading and management
- Glyph rasterization with faux bold
- Basic positioning (numpad alignment 1–9)
- `\pos`, `\move`, `\org`
- Outline, Shadow effects
- Fade (`\fad`, `\1a`–`\4a`)
- `\clip` / `\iclip`
- Drawing mode (`\p1` vector paths)
- Box blur (`\be`, `\blur`)
- Bold/Italic/Underline/Strikeout
- WebAssembly bindings
- Canvas rendering

### Not Yet Implemented
- `\t` (animated override tags)
- 3D rotation (`\frx`, `\fry`)
- Karaoke (`\k`, `\K`, `\kf`, `\ko`)
- HarfBuzz/OpenType complex shaping
- Web Worker architecture
- `[Fonts]` section embedding
- `[Graphics]` section
- SSA v4.00 (non-plus) style format
- SIMD optimizations

## License

MIT
