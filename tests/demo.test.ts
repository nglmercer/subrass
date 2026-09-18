// Bun tests for the demo worker protocol and dev-server path handling.
// Run: bun test
import { describe, expect, test } from "bun:test";
import { resolve } from "node:path";
import { WorkerBackend } from "../demo/shared/worker-backend.ts";
import {
  DEFAULT_PORT,
  decodePath,
  mimeFor,
  resolveContained,
  resolvePort,
} from "../server.ts";

describe("WorkerBackend failure paths", () => {
  test("loadAss rejects when the worker was never started", async () => {
    const backend = new WorkerBackend(new URL("http://localhost/worker.js"));
    await expect(backend.loadAss("content")).rejects.toThrow();
  });

  test("loadAss rejects after dispose", async () => {
    const backend = new WorkerBackend(new URL("http://localhost/worker.js"));
    backend.dispose();
    await expect(backend.loadAss("content")).rejects.toThrow(/disposed|not running/);
  });

  test("render/resize/loadFont are safe no-ops without a worker", () => {
    const backend = new WorkerBackend(new URL("http://localhost/worker.js"));
    expect(() => backend.renderFrame(100)).not.toThrow();
    expect(() => backend.resize(320, 180)).not.toThrow();
    expect(() => backend.loadFont("f", new Uint8Array(0))).not.toThrow();
    backend.dispose();
    expect(() => backend.renderFrame(100)).not.toThrow();
  });
});

describe("server path containment", () => {
  const demoRoot = resolve("./demo");

  test("plain traversal escapes are rejected", () => {
    expect(resolveContained(demoRoot, "../../Cargo.toml")).toBeNull();
    expect(resolveContained(demoRoot, "..\\..\\Cargo.toml")).toBeNull();
    expect(resolveContained(demoRoot, "/etc/passwd")).toBeNull();
  });

  test("encoded and double-encoded traversals are rejected", () => {
    expect(resolveContained(demoRoot, "%2e%2e/%2e%2e/Cargo.toml")).toBeNull();
    expect(resolveContained(demoRoot, "%252e%252e/%252e%252e/Cargo.toml")).toBeNull();
    expect(decodePath("%252e%252e")).toBe("..");
  });

  test("legitimate files resolve", () => {
    const hit = resolveContained(demoRoot, "sample.ass");
    expect(hit).not.toBeNull();
    expect(hit!.endsWith("sample.ass")).toBe(true);
    expect(resolveContained(demoRoot, "does-not-exist.ass")).toBeNull();
  });

  // Plan #83: every vector below would hit a REAL file
  // (package.json, Cargo.toml, server.ts) if containment failed,
  // so null proves the escape was blocked rather than merely missing.
  test("separator and encoding variants cannot escape", () => {
    const vectors = [
      "../package.json",
      "..\\package.json",
      "%2e%2e/package.json",
      "%2e%2e%2fpackage.json",
      "%2e%2e%5cpackage.json",
      "%2E%2E%2FPACKAGE.JSON",
      "..%2fpackage.json",
      "..%5cpackage.json",
      "sub/../../package.json",
      "foo/..%2f../package.json",
      "%252e%252e/package.json",
      "%25252e%25252e/package.json",
      "%2525252e/package.json",
    ];
    for (const v of vectors) {
      expect(resolveContained(demoRoot, v)).toBeNull(`vector: ${v}`);
    }
  });

  test("encoded nulls and absolute paths stay contained", () => {
    expect(resolveContained(demoRoot, "../package.json%00")).toBeNull();
    expect(resolveContained(demoRoot, "sample.ass%00.png")).toBeNull();
    expect(resolveContained(demoRoot, "%00")).toBeNull();
    // Absolute in-root paths still resolve (contained, not escaped).
    const hit = resolveContained(demoRoot, "/sample.ass");
    expect(hit).not.toBeNull();
  });

  test("every exposed root blocks escapes (#83)", () => {
    const pkgRoot = resolve("./pkg");
    const fontsRoot = resolve("./fonts");
    // pkg/.. and fonts/../.. both reach real repo files if unblocked.
    expect(resolveContained(pkgRoot, "../server.ts")).toBeNull();
    expect(resolveContained(pkgRoot, "%2e%2e%2fCargo.toml")).toBeNull();
    expect(resolveContained(fontsRoot, "../../package.json")).toBeNull();
    expect(resolveContained(fontsRoot, "..%5c..%2fserver.ts")).toBeNull();
  });
});

describe("server MIME types (#84)", () => {
  test("subtitle, font, and web types map correctly", () => {
    expect(mimeFor(".ass")).toBe("text/plain; charset=utf-8");
    expect(mimeFor(".ssa")).toBe("text/plain; charset=utf-8");
    expect(mimeFor(".ttf")).toBe("font/ttf");
    expect(mimeFor(".otf")).toBe("font/otf");
    expect(mimeFor(".woff")).toBe("font/woff");
    expect(mimeFor(".woff2")).toBe("font/woff2");
    expect(mimeFor(".wasm")).toBe("application/wasm");
    expect(mimeFor(".js")).toBe("text/javascript");
    expect(mimeFor(".mjs")).toBe("text/javascript");
    expect(mimeFor(".ts")).toBe("text/javascript");
    expect(mimeFor(".html")).toBe("text/html");
    expect(mimeFor(".css")).toBe("text/css");
    expect(mimeFor(".json")).toBe("application/json");
    expect(mimeFor(".png")).toBe("image/png");
  });

  test("unknown extensions fall back to octet-stream", () => {
    expect(mimeFor(".xyz")).toBe("application/octet-stream");
    expect(mimeFor("")).toBe("application/octet-stream");
  });
});

describe("server configuration", () => {
  test("default port is the centralized 8001", () => {
    expect(DEFAULT_PORT).toBe(8001);
    expect(resolvePort(undefined)).toBe(8001);
  });

  test("$PORT overrides the default", () => {
    expect(resolvePort("3000")).toBe(3000);
  });
});
