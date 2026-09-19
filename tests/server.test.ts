// HTTP-level tests for the dev server (server.ts static design).
// Run: bun test
//
// Starts the real handler on an ephemeral port and checks every
// route from the server contract: example pages, redirects,
// transpiled TypeScript, subtitle/font/artifact assets, methods,
// HEAD semantics, traversal resistance, and PORT validation.
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { connect } from "node:net";
import { existsSync } from "node:fs";
import type { Server } from "node:http";
import { createAppServer, resolvePort } from "../server.ts";

let server: Server;
let base = "";

beforeAll(async () => {
  server = createAppServer();
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });
  const addr = server.address();
  if (typeof addr !== "object" || !addr) throw new Error("no listen address");
  base = `http://127.0.0.1:${addr.port}`;
});

afterAll(async () => {
  await new Promise<void>((resolve, reject) => {
    server.close((err) => (err ? reject(err) : resolve()));
  });
});

/// Raw request-target GET over TCP: unlike fetch(), this sends `target`
/// exactly as given (no client-side dot-segment or encoding
/// normalization), so traversal vectors reach the server intact.
function rawGet(target: string): Promise<{ status: number; head: string; body: Buffer }> {
  const port = Number(new URL(base).port);
  return new Promise((resolvePromise, reject) => {
    const sock = connect(port, "127.0.0.1", () => {
      sock.write(`GET ${target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n`);
    });
    const chunks: Buffer[] = [];
    sock.on("data", (c: Buffer) => chunks.push(c));
    sock.on("error", reject);
    sock.on("close", () => {
      const raw = Buffer.concat(chunks);
      const headEnd = raw.indexOf("\r\n\r\n");
      const head = raw.subarray(0, headEnd).toString("latin1");
      const status = Number(head.split("\r\n")[0]?.split(" ")[1]);
      resolvePromise({ status, head, body: raw.subarray(headEnd + 4) });
    });
  });
}

describe("example pages and redirects", () => {
  test("GET / serves the landing page", async () => {
    const res = await fetch(`${base}/`);
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toContain("text/html");
    const body = await res.text();
    expect(body).toContain("ASS subtitle renderer");
  });

  test("GET /basic and /worker redirect to trailing slashes", async () => {
    for (const bare of ["/basic", "/worker"]) {
      const res = await fetch(`${base}${bare}`, { redirect: "manual" });
      expect(res.status).toBe(301);
      expect(res.headers.get("location")).toBe(`${bare}/`);
    }
  });

  test("GET /basic/ and /worker/ serve the example pages", async () => {
    const basic = await fetch(`${base}/basic/`);
    expect(basic.status).toBe(200);
    expect(basic.headers.get("content-type")).toContain("text/html");
    expect(await basic.text()).toContain("Basic example");

    const worker = await fetch(`${base}/worker/`);
    expect(worker.status).toBe(200);
    expect(await worker.text()).toContain("Worker example");
  });
});

describe("demo static assets", () => {
  test("shared stylesheet loads with CSS type", async () => {
    const res = await fetch(`${base}/shared/style.css`);
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toContain("text/css");
    expect((await res.text()).length).toBeGreaterThan(0);
  });

  test("TypeScript sources are transpiled to modules", async () => {
    for (const path of [
      "/shared/app.ts",
      "/basic/main.ts",
      "/worker/main.ts",
      "/worker/render-worker.ts",
    ]) {
      const res = await fetch(`${base}${path}`);
      expect(res.status).toBe(200);
      expect(res.headers.get("content-type")).toContain("text/javascript");
      const body = await res.text();
      expect(body.length).toBeGreaterThan(0);
      // Transpiled output must not leak TypeScript-only syntax.
      expect(body).not.toContain("interface ");
    }
  });

  test("subtitle sample loads as plain text", async () => {
    const res = await fetch(`${base}/sample.ass`);
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toContain("text/plain");
    expect(await res.text()).toContain("[Script Info]");
  });

  test("bundled font loads with font type", async () => {
    const res = await fetch(`${base}/fonts/DejaVuSans.ttf`);
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toBe("font/ttf");
  });
});

describe("built package assets", () => {
  // pkg/ is a build artifact (wasm-pack build runs before bun test
  // in CI); skip when it has not been built locally.
  const hasJs = existsSync("./pkg/subrass.js");
  const hasWasm = existsSync("./pkg/subrass_bg.wasm");
  (hasJs ? test : test.skip)("pkg JS loads", async () => {
    const res = await fetch(`${base}/pkg/subrass.js`);
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toContain("text/javascript");
  });
  (hasWasm ? test : test.skip)("pkg WASM loads with wasm type", async () => {
    const res = await fetch(`${base}/pkg/subrass_bg.wasm`);
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toBe("application/wasm");
  });
});

describe("errors and traversal resistance", () => {
  test("unknown files are 404", async () => {
    for (const path of ["/no-such-file.txt", "/shared/nope.ts", "/pkg/nope.js"]) {
      const res = await fetch(`${base}${path}`);
      expect(res.status).toBe(404);
    }
  });

  test("raw traversal vectors cannot escape (status + body)", async () => {
    // Every vector targets a REAL repo file (package.json); a 404 with
    // the stock body proves the escape was blocked rather than merely
    // missing, and the body check catches content leaks on any 200.
    const vectors = [
      "/../package.json",
      "/..%2fpackage.json",
      "/%2e%2e/package.json",
      "/%2e%2e%2fpackage.json",
      "/%252e%252e/package.json",
      "/%252e%252e/%252e%252e/package.json",
      "/..%5cpackage.json",
      "/..\\package.json",
      "/shared/../../package.json",
      "/shared/%2e%2e/%2e%2e/package.json",
    ];
    for (const target of vectors) {
      const { status, body } = await rawGet(target);
      expect(status).toBe(404);
      expect(body.toString("utf-8")).not.toContain("subrass-demo");
    }
  });

  test("query strings do not change routing", async () => {
    const res = await fetch(`${base}/sample.ass?v=1&x=2`);
    expect(res.status).toBe(200);
    const page = await fetch(`${base}/basic/?v=1`, { redirect: "manual" });
    expect(page.status).toBe(200);
  });
});

describe("methods and HEAD semantics", () => {
  test("unsupported methods are 405 with an Allow header", async () => {
    for (const method of ["POST", "PUT", "DELETE", "OPTIONS", "PATCH"]) {
      const res = await fetch(`${base}/sample.ass`, { method });
      expect(res.status).toBe(405);
      expect(res.headers.get("allow")).toContain("GET");
    }
  });

  test("HEAD returns headers without a body", async () => {
    const get = await fetch(`${base}/sample.ass`);
    const getBody = await get.arrayBuffer();
    const head = await fetch(`${base}/sample.ass`, { method: "HEAD" });
    expect(head.status).toBe(200);
    expect(head.headers.get("content-type")).toBe(get.headers.get("content-type"));
    expect(head.headers.get("content-length")).toBe(String(getBody.byteLength));
    expect((await head.arrayBuffer()).byteLength).toBe(0);
  });

  test("HEAD on a missing file is a bodiless 404", async () => {
    const res = await fetch(`${base}/no-such-file.txt`, { method: "HEAD" });
    expect(res.status).toBe(404);
    expect((await res.arrayBuffer()).byteLength).toBe(0);
  });
});

describe("PORT validation", () => {
  test("valid ports are accepted", () => {
    expect(resolvePort(undefined)).toBe(8001);
    expect(resolvePort("1")).toBe(1);
    expect(resolvePort("80")).toBe(80);
    expect(resolvePort("8001")).toBe(8001);
    expect(resolvePort("65535")).toBe(65535);
  });

  test("invalid ports throw instead of binding", () => {
    for (const bad of [
      "",
      "NaN",
      "abc",
      "-1",
      "0",
      "65536",
      "99999",
      "80.5",
      "8.5",
      "0x50",
      "8e3",
      "80x",
    ]) {
      expect(() => resolvePort(bad)).toThrow(RangeError);
    }
  });
});
