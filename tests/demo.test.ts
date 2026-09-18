// Bun tests for the demo worker protocol and dev-server path handling.
// Run: bun test
import { describe, expect, test } from "bun:test";
import { resolve } from "node:path";
import { WorkerBackend } from "../demo/shared/worker-backend.ts";
import { decodePath, resolveContained } from "../server.ts";

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
});
