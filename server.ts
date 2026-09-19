import { watch } from "node:fs";
import { extname, normalize, resolve, sep } from "node:path";
import { createServer as createHttpServer, type Server } from "node:http";
import { readFile } from "node:fs/promises";
import { existsSync, statSync } from "node:fs";

/// Default dev-server port (build.sh and docs must agree with this).
export const DEFAULT_PORT = 8001;

/// Resolve the listen port: `$PORT` when set, otherwise [DEFAULT_PORT].
///
/// Strictly validated: a valid port is all digits in `1..=65535`.
/// Anything else (empty, `NaN`, negatives, decimals, out-of-range)
/// throws a `RangeError` instead of binding a surprising port.
export function resolvePort(envPort: string | undefined): number {
  if (envPort === undefined) return DEFAULT_PORT;
  const text = envPort.trim();
  if (!/^\d+$/.test(text)) {
    throw new RangeError(`Invalid PORT ${JSON.stringify(envPort)}: expected an integer 1..65535`);
  }
  const port = Number(text);
  if (!Number.isSafeInteger(port) || port < 1 || port > 65535) {
    throw new RangeError(`Invalid PORT ${JSON.stringify(envPort)}: expected an integer 1..65535`);
  }
  return port;
}

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

/// Example pages keyed by canonical path (trailing slash, so the
/// relative `./main.ts` module URLs in the pages resolve correctly).
const EXAMPLE_PAGES = new Map([
  ["/", DEMO_INDEX],
  ["/basic/", "./demo/basic/index.html"],
  ["/worker/", "./demo/worker/index.html"],
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

/// Create the dev-server request handler (exported for tests).
///
/// Serves, in order:
/// ```text
/// /                      demo landing page
/// /basic/ /worker/       example pages (bare /basic and /worker
///                        redirect here with 301 so relative module
///                        URLs resolve)
/// /pkg/* /fonts/*        built artifacts and bundled fonts
/// demo static files      /shared/*, /basic/*, /worker/*, /sample.ass
///                        (.ts transpiled on the fly, everything else
///                        by extension MIME)
/// ```
/// Only `GET` and `HEAD` are served; anything else gets `405`.
/// `HEAD` returns headers (including `Content-Length`) without a body.
export function createAppServer(): Server {
  return createHttpServer(async (req, res) => {
    try {
      const method = req.method ?? "GET";
      if (method !== "GET" && method !== "HEAD") {
        res.writeHead(405, { Allow: "GET, HEAD" });
        res.end("Method not allowed");
        return;
      }
      const headOnly = method === "HEAD";
      const send = (status: number, headers: Record<string, string>, body?: string | Buffer) => {
        const payload = body === undefined ? Buffer.alloc(0) : Buffer.from(body);
        res.writeHead(status, { ...headers, "Content-Length": String(payload.length) });
        res.end(headOnly ? undefined : payload);
      };

      const url = new URL(req.url ?? "/", `http://${req.headers.host}`);
      const pathname = url.pathname;

      // Canonicalize example URLs: without the trailing slash the
      // pages' relative `./main.ts` would resolve to `/main.ts`.
      if (pathname === "/basic" || pathname === "/worker") {
        res.writeHead(301, { Location: `${pathname}/` });
        res.end();
        return;
      }

      // Example pages.
      const examplePath = EXAMPLE_PAGES.get(pathname);
      if (examplePath) {
        const data = await readFile(examplePath, "utf-8");
        send(200, { "Content-Type": "text/html" }, data);
        return;
      }

      // Built JS/WASM artifacts and bundled fonts, each contained to
      // its own root (never the demo tree or the repo at large).
      for (const [prefix, root] of [
        ["/pkg/", PKG_ROOT],
        ["/fonts/", FONTS_ROOT],
      ] as const) {
        if (pathname.startsWith(prefix)) {
          const filePath = resolveContained(root, pathname.slice(prefix.length));
          if (!filePath) {
            send(404, {}, "Not found");
            return;
          }
          const data = await readFile(filePath);
          send(200, { "Content-Type": mimeFor(extname(filePath).toLowerCase()) }, data);
          return;
        }
      }

      // Generic demo static files: /shared/*, /basic/*, /worker/*,
      // /sample.ass. TypeScript sources are transpiled on the fly so
      // the browser receives plain modules; everything else is served
      // by extension MIME. Containment keeps this inside demo/.
      const filePath = resolveContained(DEMO_ROOT, pathname);
      if (!filePath) {
        send(404, {}, "Not found");
        return;
      }
      const ext = extname(filePath).toLowerCase();
      if (ext === ".ts") {
        const transpiler = new Bun.Transpiler({ loader: "ts" });
        const source = await readFile(filePath, "utf-8");
        const js = transpiler.transformSync(source);
        send(200, { "Content-Type": "text/javascript" }, js);
        return;
      }
      const data = await readFile(filePath);
      send(200, { "Content-Type": mimeFor(ext) }, data);
    } catch (error) {
      console.error("Server error:", error);
      if (!res.headersSent) res.writeHead(500);
      res.end(req.method === "HEAD" ? undefined : "Internal server error");
    }
  });
}

if (import.meta.main) {
  let port: number;
  try {
    port = resolvePort(process.env.PORT);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  }
  const server = createAppServer();
  server.listen(port, () => {
    console.log(`Dev server running at http://localhost:${port}`);
    console.log(`  / ........... ${DEMO_INDEX}`);
    console.log(`  /basic/ ..... ./demo/basic/index.html`);
    console.log(`  /worker/ .... ./demo/worker/index.html`);
  });
}

// Watch for changes and log them (Bun auto-restarts with --watch flag)
if (process.env.BUN_WATCH === "1") {
  watch("./demo", { recursive: true }, (_event, filename) => {
    console.log(`File changed: ${filename}`);
  });
}
