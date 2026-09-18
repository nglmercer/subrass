import { watch } from "node:fs";
import { extname, join, normalize, resolve, sep } from "node:path";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { existsSync, statSync } from "node:fs";

/// Default dev-server port (build.sh and docs must agree with this).
export const DEFAULT_PORT = 8001;

/// Resolve the listen port: `$PORT` when set, otherwise [DEFAULT_PORT].
export function resolvePort(envPort: string | undefined): number {
  return Number(envPort ?? DEFAULT_PORT);
}

const PORT = resolvePort(process.env.PORT);
const DEMO_INDEX = "./demo/index.html";

const MIME_TYPES: Record<string, string> = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".ts": "text/javascript",
  ".wasm": "application/wasm",
  ".ass": "text/plain; charset=utf-8",
  ".ssa": "text/plain; charset=utf-8",
  ".css": "text/css",
  ".json": "application/json",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".bmp": "image/bmp",
  ".webp": "image/webp",
  ".ttf": "font/ttf",
  ".otf": "font/otf",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
};

const EXAMPLE_PAGES = new Map([
  ["/", DEMO_INDEX],
  ["/basic", "./demo/basic/index.html"],
  ["/worker", "./demo/worker/index.html"],
]);

/// Content type for a file extension (leading dot, lowercase).
/// Unknown extensions fall back to `application/octet-stream`.
export function mimeFor(ext: string): string {
  return MIME_TYPES[ext] ?? "application/octet-stream";
}

const DEMO_ROOT = resolve("./demo");
const PKG_ROOT = resolve("./pkg");
const FONTS_ROOT = resolve("./fonts");

/// Decode a URL path repeatedly (bounded) so double-encoded sequences
/// cannot smuggle separators past the containment check below.
export function decodePath(raw: string): string {
  let decoded = raw;
  for (let i = 0; i < 3; i++) {
    try {
      const next = decodeURIComponent(decoded);
      if (next === decoded) break;
      decoded = next;
    } catch {
      break;
    }
  }
  return decoded;
}

/// Resolve `requestPath` against `root`, returning null when the result
/// escapes the root (path traversal) or is not a regular file.
export function resolveContained(root: string, requestPath: string): string | null {
  const rel = normalize(decodePath(requestPath)).replace(/^([/\\])+/, "");
  const abs = resolve(root, rel);
  if (abs !== root && !abs.startsWith(root + sep)) return null;
  try {
    if (!existsSync(abs) || !statSync(abs).isFile()) return null;
  } catch {
    return null;
  }
  return abs;
}

const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url ?? "/", `http://${req.headers.host}`);
    const pathname = url.pathname;

    // Serve files from pkg/ (compiled JS/WASM/text artifacts)
    if (pathname.startsWith("/pkg/")) {
      const filePath = resolveContained(PKG_ROOT, pathname.slice("/pkg/".length));
      if (!filePath) {
        res.writeHead(404);
        res.end("Not found");
        return;
      }
      const data = await readFile(filePath);
      res.writeHead(200, { "Content-Type": mimeFor(extname(filePath)) });
      res.end(data);
      return;
    }

    // Serve font files so demos can fetch the bundled fonts
    if (pathname.startsWith("/fonts/")) {
      const filePath = resolveContained(FONTS_ROOT, pathname.slice("/fonts/".length));
      if (!filePath) {
        res.writeHead(404);
        res.end("Not found");
        return;
      }
      const data = await readFile(filePath);
      res.writeHead(200, { "Content-Type": mimeFor(extname(filePath)) });
      res.end(data);
      return;
    }

    // Serve .ts example sources compiled on the fly by Bun
    if (pathname.endsWith(".ts")) {
      const filePath = resolveContained(DEMO_ROOT, pathname);
      if (!filePath) {
        res.writeHead(404);
        res.end("Not found");
        return;
      }
      const transpiler = new Bun.Transpiler({ loader: "ts" });
      const source = await readFile(filePath, "utf-8");
      const js = transpiler.transformSync(source);
      res.writeHead(200, { "Content-Type": "text/javascript" });
      res.end(js);
      return;
    }

    // Subtitle sample
    if (pathname === "/sample.ass") {
      const data = await readFile("./demo/sample.ass");
      res.writeHead(200, { "Content-Type": mimeFor(".ass") });
      res.end(data);
      return;
    }

    // Serve example pages
    const examplePath = EXAMPLE_PAGES.get(pathname);
    if (examplePath) {
      const data = await readFile(examplePath, "utf-8");
      res.writeHead(200, { "Content-Type": "text/html" });
      res.end(data);
      return;
    }

    res.writeHead(404);
    res.end("Not found");
  } catch (error) {
    console.error("Server error:", error);
    res.writeHead(500);
    res.end("Internal server error");
  }
});

if (import.meta.main) {
  server.listen(PORT, () => {
    console.log(`Dev server running at http://localhost:${PORT}`);
    console.log(`  / ........... ${DEMO_INDEX}`);
    console.log(`  /basic ...... ./demo/basic/index.html`);
    console.log(`  /worker ..... ./demo/worker/index.html`);
  });
}

// Watch for changes and log them (Bun auto-restarts with --watch flag)
if (process.env.BUN_WATCH === "1") {
  watch("./demo", { recursive: true }, (_event, filename) => {
    console.log(`File changed: ${filename}`);
  });
}
