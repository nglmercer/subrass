# Subrass — Implement All Remaining Features

## P0 — Rebuild `\t(...)` to match libass

Current transform handling is still architecturally incomplete.

### Event-global state

Tags inside `\t` that affect the whole event must influence final rendering, not only a segment-local `ResolvedStyle`.

Fix render-level behavior for:

```text
\pos \move \org
\an \a
\q
\fad \fade
\clip \iclip
```

Ensure final:

```text
positioning
origin
wrap
fade
clip
alignment
layout
```

use the transformed state at the current timestamp.

Do not merely assert fields on a temporary segment object.

### Nested transforms

Nested `\t` must evaluate its own:

```text
t1
t2
accel
progress
```

Do not reuse the outer transform's progress blindly.

Implement bounded recursive evaluation matching current libass.

### Rectangular clip transforms

Remove hard-coded:

```rust
(0, 0, 384, 288)
```

The initial rectangle must be:

```text
(0, 0, PlayResX, PlayResY)
```

Use actual script resolution.

Match libass integer conversion exactly when interpolating clip coordinates; do not use `.round()` if libass truncates.

Test:

```text
non-384x288 PlayRes
clip inside \t
iclip inside \t
existing clip -> transformed clip
nested transformed clips
early/mid/late timestamps
```

### Transform tags

Verify all tags recursively against libass rather than using assumptions.

Cover at least:

```text
\b \i \u \s
\fn \fe
\fs \fsp
\fscx \fscy
\fr \frx \fry \frz
\fax \fay
\bord \xbord \ybord
\shad \xshad \yshad
\be \blur
colors/alpha
\p \pbo
\q
karaoke
alignment
positioning
clips
resets
nested \t
```

Add gated libass fixtures for every meaningful transform class.

---

## P0 — Complete libass numeric/tag parsing

Use libass-compatible prefix parsing consistently for every override tag.

Match:

```text
whitespace
+ / -
decimal
exponent
numeric prefix + garbage suffix
empty value
bare tag
overflow
underflow
invalid numeric input
property reset semantics
```

Examples:

```ass
{\b1foo}x
{\frz30xyz}x
{\bord2abc}x
{\q2junk}x
{\fs40foo}x
{\p2garbage}...
```

Do not maintain a second strict-Rust parsing behavior where libass would accept a numeric prefix.

Add parser and render references.

---

## P1 — Finish `\pbo`

Match libass drawing baseline metrics exactly.

Do not clamp drawing ascent/baseline unless libass does.

Cover:

```text
positive pbo
negative pbo
very large pbo
single drawing line
mixed text + drawing
multiple drawings
multiple lines
alignment 1-9
scaled PlayRes
\r interaction
\t interaction
```

Add gated libass fixtures.

---

## P1 — Implement real text shaping

Replace scalar-only shaping with proper OpenType shaping.

Use:

```text
rustybuzz
unicode-bidi or equivalent
```

Required:

```text
Arabic contextual shaping
Hebrew / RTL
mixed LTR + RTL
Indic shaping
ligatures
kerning
mark positioning
combining clusters
glyph substitution
glyph positioning
visual-order runs
cluster mapping
```

One shaping pipeline must feed:

```text
measurement
wrapping
layout
rendering
karaoke
decorations
fallback
hit/span accounting
```

Do not measure with one algorithm and render with another.

### Font fallback

Make fallback cluster/run aware.

Never split a shaped cluster across incompatible fonts unless unavoidable.

Add references for:

```text
Arabic
Hebrew
mixed bidi
Devanagari/Indic
combining marks
fi/fl ligatures
kerning pairs
fallback across shaped runs
karaoke over shaped clusters
```

---

## P1 — Implement `\fe`

Implement charset behavior needed for ASS/SSA compatibility.

Support relevant legacy charset/codepage mapping instead of treating `\fe` as render-neutral metadata.

Preserve Unicode input safely.

Add tests/references for charset switches and resets.

Only mark `\fe` Supported when rendering behavior is implemented.

---

## P1 — Font collections

Implement `.ttc` / `.otc` support.

Required:

```text
enumerate collection faces
read family/style/weight metadata per face
match requested ASS family
match weight/italic
deterministic fallback
stable face identity/cache keys
```

Do not silently choose face 0.

Keep `.ttf`/`.otf` behavior unchanged.

---

## P2 — System font discovery

If full desktop/libass compatibility is a project goal, implement host system-font lookup behind a platform-specific/native feature.

Requirements:

```text
WASM remains deterministic and sandboxed
native builds may discover system fonts
loaded/embedded fonts take deterministic precedence
explicit load_font remains supported
```

Do not make browser/WASM output depend on host-installed fonts.

---

## P2 — Complete ASS script fields

Implement currently parsed-but-unused fields where applicable:

```text
LayoutResX
LayoutResY
YCbCr Matrix
```

Match libass coordinate/layout behavior.

For YCbCr handling, ensure color conversion is intentional and reference-tested.

---

## P2 — Exact vector drawing behavior

Audit ASS drawing parity:

```text
m n l b s p c
```

Replace approximation where needed, especially B-spline handling.

Match libass/VSFilter geometry, winding/fill, bounds, scaling, baseline and clipping.

Add complex drawing fixtures.

---

## P2 — Remaining rendering parity

Re-audit and resolve or explicitly test:

```text
blur accumulation/order
perspective transforms
glyph overlap
underline/strikeout
BorderStyle=3
karaoke + transforms
vector clips
legacy Banner/Scroll
CJK wrapping
wrap styles
font fallback
drawing + karaoke
```

If intentionally targeting VSFilter rather than libass for a feature, document that explicitly.

---

## P2 — Reference suite expansion

Current references are not sufficient for the newest transform work.

Add gated libass fixtures for:

```text
transform-pos
transform-org
transform-an
transform-q
transform-fade
transform-clip
transform-iclip
transform-discrete
transform-karaoke
transform-nested
transform-nested-accel
transform-nondefault-playres
pbo-positive
pbo-negative
pbo-mixed
numeric-prefix-tags
arabic
hebrew
mixed-bidi
indic
ligature
kerning
font-collection
fe-charset
```

Requirements:

```text
pending = 0
```

Do not weaken thresholds to make results pass.

Keep blank-frame/reference-integrity checks.

---

## P3 — Parser/render safety

Maintain strict resource limits and bounded work.

Audit:

```text
nested \t recursion
drawing complexity
shaping cluster count
font collections
font table parsing
bidi runs
clip geometry
bitmap dimensions
integer conversions
allocation multiplication
WASM32 usize limits
```

Keep all document-wide caps and attachment decode budgets.

No unbounded recursive parsing.

---

## P3 — Server

Do not regress the already-fixed dev server.

Keep tests for:

```text
/
/basic -> /basic/
/worker -> /worker/
/shared/*
/basic/*
/worker/*
/pkg/*
/fonts/*
HEAD
405
traversal protection
double encoding
PORT validation
```

---

## P3 — Documentation

Synchronize:

```text
README.md
CONFORMANCE.md
AUDIT_FIXES.md
fuzz/README.md
```

Fix stale reference counts, including any remaining `3 known-divergent` vs current `7 known-divergent` mismatch.

The feature matrix must reflect actual implementation.

Do not claim full compatibility while anything remains Partial/Unsupported.

---

## Verification

Run:

```bash
cargo fmt --all -- --check

cargo clippy \
  --all-targets \
  --all-features \
  --locked \
  -- \
  -D warnings

cargo test --locked --all-features
cargo test --locked --no-default-features

cargo check \
  --target wasm32-unknown-unknown \
  --tests \
  --locked

cargo test --locked --test golden

SUBRASS_STRICT_REFERENCES=1 \
cargo test --locked --test reference -- --nocapture

wasm-pack build --target web --out-dir pkg
wasm-pack test --headless --chrome

bun install --frozen-lockfile
bun run typecheck
bun test

cargo audit --deny warnings
```

Run fuzz smoke:

```bash
cd fuzz

cargo fuzz build
cargo fuzz run parse_ass -- -runs=10000
cargo fuzz run drawing -- -runs=10000
cargo fuzz run render -- -runs=10000
```

Add shaping/font fuzz coverage if new parsers are introduced.

---

# Definition of done

Do not say "all features implemented" until:

```text
render-level \t behavior matches libass
nested \t timing/accel works
transformed clips use actual PlayRes
clip interpolation conversion matches libass
numeric parsing matches libass
\pbo matches libass
complex shaping works
RTL/bidi works
Indic/Arabic shaping works
ligatures/kerning/marks work
\fe charset behavior works
TTC/OTC collections work
script layout/color fields are implemented
drawing behavior is reference-backed
new functionality has libass fixtures
pending references = 0
all Rust/WASM/Bun/reference/fuzz/audit checks pass
docs contain no unsupported feature described as Supported
```

At completion, provide a final table with only:

```text
Feature
Implementation status
Reference coverage
Known intentional divergence
```

Any intentionally unsupported behavior must remain explicitly listed instead of being hidden behind “all tests pass.”
