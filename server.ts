// Dev server for the subrass demo.
//
// Serves the example pages with TypeScript transpiled on the fly (no build
// step), plus the wasm-pack output in pkg/ and the sample subtitles:
//
//   wasm-pack build --target web   # once, to produce pkg/
//   bun run server.ts              # http://localhost:3001
//
// Routes:
//   /         landing page (demo/index.html)
//   /basic    main-thread rendering example
//   /worker   Web Worker rendering example
//   /pkg/*    wasm-pack build output
//   /*        everything else resolves under demo/

import { normalize, resolve } from "node:path";

const ROOT = import.meta.dir;
const DEMO = resolve(ROOT, "demo");

const MIME: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".ts": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".wasm": "application/wasm",
  ".ass": "text/plain; charset=utf-8",
  ".ssa": "text/plain; charset=utf-8",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
};

const transpiler = new Bun.Transpiler({ loader: "ts" });

/** Map a URL pathname to a file on disk, or null if outside the allowed roots. */
function resolvePath(pathname: string): string | null {
  if (pathname === "/") return resolve(DEMO, "index.html");
  if (pathname === "/basic") return resolve(DEMO, "basic/index.html");
  if (pathname === "/worker") return resolve(DEMO, "worker/index.html");

  let decoded: string;
  try {
    decoded = decodeURIComponent(pathname);
  } catch {
    return null;
  }
  const rel = normalize(decoded).replace(/^[/\\]+/, "");
  if (rel === "" || rel.startsWith("..")) return null;

  // The WASM package lives at the repo root; everything else under demo/.
  if (rel.startsWith("pkg/") || rel.startsWith("fonts/")) return resolve(ROOT, rel);
  return resolve(DEMO, rel);
}

const server = Bun.serve({
  port: 3001,
  async fetch(req) {
    const url = new URL(req.url);
    const file = resolvePath(url.pathname);
    if (!file) return new Response("Not Found", { status: 404 });

    const ext = file.slice(file.lastIndexOf("."));
    const contentType = MIME[ext];
    if (!contentType) return new Response("Not Found", { status: 404 });

    const handle = Bun.file(file);
    if (!(await handle.exists())) return new Response("Not Found", { status: 404 });

    const headers = {
      "Content-Type": contentType,
      "Cache-Control": "no-cache",
    };
    if (ext === ".ts") {
      return new Response(transpiler.transformSync(await handle.text()), { headers });
    }
    return new Response(handle, { headers });
  },
});

console.log(`subrass demo: http://localhost:${server.port}`);
