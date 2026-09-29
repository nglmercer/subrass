# Demo screenshot investigation

Reviewed compatibility commit `b17e6e1` and the main-thread/worker demo code.
The two supplied screenshots show normal authored content. The fixes below
address separate demo failures found while investigating them.

## Screenshot results

| Screenshot time | Authored events | Expected appearance |
|---|---|---|
| 1:08.109 | `demo/sample.ass` color scene, 1:05–1:09: center alignment at `(960,300)` and `(960,370)` | Red/green/blue on the upper row; magenta fill and yellow outline on the lower row. With a 1920x1080 script displayed at about 1036x583, the nominal 48px font becomes about 26px. The empty space is authored positioning. |
| 4:49.345 | Layer scene, 4:45–4:50: layers 0/1/2, center positions `(960,400)`, `(960,420)`, `(960,440)`, font sizes 60/50/40 | Blue, green, red labels intentionally overlap. “Bottom/middle/top” means behind/middle/front in painting order. All three are explicitly positioned and excluded from collision placement; separate layers also do not collide. |

Both timestamps were rendered directly with libass 0.17.5, complex shaping,
no hinting, storage=frame, pixel aspect 1, Unicode wrapping off, and the committed
DejaVu Sans as the sole font provider. Native subrass and the reference were
compared at 1036x583. Their colors, placement, sizes, and overlap agree;
rasterization/outline coverage has the previously documented small differences.
The reference's Arial request uses DejaVu fallback, matching the demo's built-in
font. This comparison does not claim exact Arial metrics or full libass parity.

Comparison images and raw frames from the investigation are in
`/tmp/subrass-demo-check/` (`comparison-68109.png`, `comparison-289345.png`;
subrass left, libass right). These are debugging artifacts, not regenerated
goldens. Existing conformance thresholds and reference fixtures are unchanged.

## Confirmed demo fixes

- Virtual `Player.pause()` disabled the playing flag before sampling elapsed
  time. A regression reproduced 68.109 seconds reverting to 68.000. Pause now
  samples the running clock first; resume continues from that captured time.
- Main-thread subtitle reload created a renderer at its new PlayRes while the
  canvas could remain at the previous output size. It now retains the explicit
  output dimensions and releases the old renderer after a successful replacement.
- Failed replacement subtitles could leave the app document or worker renderer
  pointing to an already freed WASM object. Both paths now construct and validate
  the replacement first, free rejected candidates, and keep valid existing state
  on failure. The filename changes only after a successful load. Failed worker
  size changes also preserve the size used for subsequent subtitle loads.
- Uploads called `File.text()`, losing UTF-16 and legacy source bytes before they
  reached the parser. Both backends now accept strings or raw byte arrays;
  uploads use `arrayBuffer()` and the document/renderer `from_bytes` APIs.
- The complex-fade sample passed eight arguments to a seven-argument tag, so it
  was ignored. The extra zero was removed from the override and its explanatory
  label. The authored alpha values and valid timing phases were preserved.

## Tests and checks

New regressions are `tests/player.test.ts`, `tests/direct-backend.test.ts`,
`tests/render-worker.test.ts`, `tests/app.test.ts`, and `tests/demo_render.rs`.
The main backend tests use the real WASM parser/renderer and a substituted canvas
surface. The worker test runs the actual render worker under Bun. The app wiring
regression uses DOM substitutes with the real document parser. These supplement
browser engine tests; they are not a claim of interactive UI coverage.

| Command / check | Result |
|---|---|
| `bun run build` | Optimized WASM package built successfully |
| `bun run typecheck` | Pass |
| `bun test` | 48 passed, 236 assertions, no failures |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | Pass |
| `cargo test --locked --all-features` | 472 passed; existing benchmark ignored |
| `SUBRASS_STRICT_REFERENCES=1 cargo test --locked --test reference -- --nocapture` | 9 tests pass; 180 unchanged frame gates pass, no pending/failures |
| `cargo test --locked --test demo_render -- --nocapture` | 3 regressions pass; 110 sample events covered at 232 unique event boundaries/midpoints |
| Headless Chrome WASM tests | 40 passed |
| Live demo HTTP checks | Main-thread/worker pages, modules, sample, and WASM served successfully |

The browser command used the existing matching ChromeDriver/Chromium configuration:

```sh
mkdir -p target/demo-check-tmp
TMPDIR="$PWD/target/demo-check-tmp" \
WASM_BINDGEN_TEST_WEBDRIVER_JSON=/tmp/subrass-reference/webdriver.json \
wasm-pack test --headless --chrome \
  --chromedriver /tmp/subrass-reference/chromedriver-linux64/chromedriver
```

The first attempt stopped because the host `/tmp` filesystem was full before
browser tests started. Repeating with a workspace temporary directory passed.
No other temporary files were deleted to make the test pass. Native/demo logs
are under `/tmp/subrass-demo-check/`; browser output is `target/demo-check-chrome.log`.

The demo remains served at `http://localhost:8001/basic/` and `/worker/`.
Refresh the page to load updated TypeScript modules and the corrected sample.
No connected browser UI was available for automated visual interaction, and no
real video file was supplied for media playback testing. The supplied images,
direct reference renders, real WASM/worker tests, source inspection, and sample
boundary sweep support the findings above. General rendering limitations remain
in [CONFORMANCE.md](CONFORMANCE.md).

## Playground redesign and hover inspection

A later follow-up replaces the large centered header and file buttons with a
compact responsive playground, a wider preview, sample scene shortcuts, and
active event cards below the player. The two screenshots still represent the
same authored sample; this redesign changes the interface, not the subtitles.

Enable **Inspect on hover**, or open `/basic/?inspect` or `/worker/?inspect`.
Hover the preview to select the last painted overlapping layout box. Focus or
tap an active event card to inspect any specific layer. The side panel retains
the last selected active event when the pointer leaves, allowing scrolling and
copying. Pause playback to inspect a stable frame. Turning inspection off clears
metadata and overlays. `?debug` remains a separate console logging option.

The inspector shows event layer/style/timing/actor/effect/margins, original ASS
text and tags, alignment, pivot, collision eligibility/displacement, fade opacity,
and effective per-run font request/size, fill/outline RGBA, X/Y scale and shear,
X/Y/Z rotation, border, shadow, Gaussian blur and edge blur. Frame metadata comes
from the renderer's measured layout after collision placement and is delivered
with the frame actually painted, including the worker request ID/timestamp path.

This is layout inspection, not pixel picking. Rectangles include borders but
precede rotation, shear, perspective, clipping, shadow and blur. Invisible/faded
runs can retain layout boxes. Font names identify requested families, not the
fallback face selected for every glyph. RGBA values are requested run colors;
karaoke coverage, fade and clipping modify their eventual pixels. Snapshots are
opt-in and capped at 256 events, 64 runs/event and 256 Unicode scalars per font
name. The preview draws at most 64 boxes, while event cards can select any event
in the retained snapshot. Source text display is capped at 4000 characters.
Native renderer callers can use `set_debug_enabled` and `frame_debug`; WASM callers
use `set_debug_enabled` and `get_frame_debug` after rendering.

### Reproducible browser checks

Build and serve the demo, start a compatible ChromeDriver, then run:

```sh
bun run build
bun run start
# In another terminal, with Chrome/Chromium installed:
chromedriver --port=9515
# In another terminal:
bun run test:demo:browser
```

The standalone browser regression runner accepts `WEBDRIVER_URL` (default
`http://localhost:9515`), `DEMO_URL` (default `http://localhost:8001`), optional
`CHROME_BINARY`, `CHROME_NO_SANDBOX=1` for a container that requires it, and
`DEMO_ARTIFACT_DIR` (default `target/demo-browser`). It closes its browser session
and saves desktop/hover/layer/mobile PNGs. It tests both live pages with actual
WASM and worker rendering, pointer hover, keyboard focus, retained details,
inline run properties, unchanged canvas pixels, disabled-inspector cleanup,
mobile overflow, UTF-16 file input, and state preservation on invalid uploads.
It supplements `bun test` and the Rust WASM browser suite.

For this host, Chrome/ChromeDriver 153.0.8010.12 were used. `/tmp` was full and the
first Chrome session crashed; starting ChromeDriver with a workspace `TMPDIR`
resolved it without deleting other files:

```sh
mkdir -p target/demo-browser-tmp
TMPDIR="$PWD/target/demo-browser-tmp" \
  /tmp/subrass-reference/chromedriver-linux64/chromedriver --port=9516
WEBDRIVER_URL=http://localhost:9516 \
CHROME_BINARY=/home/meme/.cache/ms-playwright/chromium-1243/chrome-linux64/chrome \
CHROME_NO_SANDBOX=1 bun run test:demo:browser
```

### Follow-up validation

| Command / check | Result |
|---|---|
| `bun run build` | Optimized WASM package built |
| `bun run typecheck` | Pass |
| `bun test` | 52 passed, no failures |
| `bun run test:demo:browser` with the environment above | Both backends pass; desktop and mobile screenshots visually reviewed |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | Pass |
| `cargo test --locked --all-features` | 475 passed, existing benchmark ignored |
| `cargo test --locked --no-default-features` | Pass |
| `SUBRASS_STRICT_REFERENCES=1 cargo test --locked --test reference` | All 180 existing frame gates pass; no threshold or golden changes |
| Headless Chrome WASM command above, using `target/demo-browser-tmp` as `TMPDIR` | 41 passed, including new debug export regression |

New renderer regressions cover opt-in clearing, pixel preservation, deterministic
seeking, inline colors, animated run values, collision displacement, stable source
indices including comments, and metadata caps. Logs for this follow-up are
`target/demo-redesign-*.log`; screenshots are `target/demo-browser/*.png`.
A real video playback/upload session and other browser engines were not exercised
in this follow-up. Headless Chrome did exercise the actual demo UI; the earlier
section's lack of connected interactive browser tooling still applies only to
that earlier investigation.
