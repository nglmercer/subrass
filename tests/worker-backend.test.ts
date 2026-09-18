// WorkerBackend protocol tests with a fake Worker (plan #79-81).
// Run: bun test
import { describe, expect, test } from "bun:test";
import { WorkerBackend } from "../demo/shared/worker-backend.ts";

/** Minimal fake Worker: records posts, replays inbound messages. */
class FakeWorker {
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onerror: ((event: { message: string }) => void) | null = null;
  onmessageerror: (() => void) | null = null;
  posted: Array<{ message: Record<string, unknown>; transfer: unknown }> = [];
  terminated = false;

  postMessage(message: Record<string, unknown>, transfer?: unknown): void {
    this.posted.push({ message, transfer });
  }

  terminate(): void {
    this.terminated = true;
  }

  emit(data: unknown): void {
    this.onmessage?.({ data });
  }

  renders(): Array<Record<string, unknown>> {
    return this.posted.map((p) => p.message).filter((m) => m.kind === "render");
  }
}

function readyBackend(onError?: (message: string) => void): {
  backend: WorkerBackend;
  fake: FakeWorker;
  ready: Promise<void>;
} {
  const fake = new FakeWorker();
  const backend = new WorkerBackend(new URL("http://localhost/worker.js"), {
    onError,
    createWorker: () => fake as unknown as Worker,
  });
  const ready = backend.init();
  return { backend, fake, ready };
}

/** Wait for init() to spawn the worker, complete the handshake. */
async function handshake(fake: FakeWorker, ready: Promise<void>): Promise<void> {
  for (let i = 0; i < 200 && fake.posted.length === 0; i++) {
    await new Promise((r) => setTimeout(r, 5));
  }
  expect(fake.posted).toHaveLength(1);
  fake.emit({ kind: "ready" });
  await ready;
}

describe("worker disposal (#79)", () => {
  test("dispose during init rejects init and never spawns", async () => {
    const { backend, fake, ready } = readyBackend();
    const settled = ready.then(
      () => "resolved",
      () => "rejected",
    );
    backend.dispose();
    expect(await settled).toBe("rejected");
    // init() was still awaiting wasm: no worker may be spawned after.
    await new Promise((r) => setTimeout(r, 50));
    expect(fake.posted).toHaveLength(0);
    await expect(backend.loadAss("x")).rejects.toThrow(/not running/);
  });

  test("dispose during load rejects every pending load", async () => {
    const { backend, fake, ready } = readyBackend();
    await handshake(fake, ready);
    const a = backend.loadAss("A").then(
      () => "resolved",
      () => "rejected",
    );
    const b = backend.loadAss("B").then(
      () => "resolved",
      () => "rejected",
    );
    backend.dispose();
    expect(await a).toBe("rejected");
    expect(await b).toBe("rejected");
    expect(fake.terminated).toBe(true);
  });

  test("dispose during render drops in-flight and pending state", async () => {
    const { backend, fake, ready } = readyBackend();
    await handshake(fake, ready);
    backend.renderFrame(100);
    backend.renderFrame(200); // pending
    expect(fake.renders()).toHaveLength(1);
    backend.dispose();
    // Late frame for the dead render is ignored, not re-sent.
    fake.emit({ kind: "frame", requestId: 1, w: 2, h: 2, bytes: new ArrayBuffer(16) });
    expect(fake.renders()).toHaveLength(1);
    backend.renderFrame(300);
    expect(fake.renders()).toHaveLength(1);
  });

  test("no promise is left unsettled by dispose", async () => {
    const { backend, fake, ready } = readyBackend();
    // Attach first, dispose while init is still waiting on wasm, then
    // every promise must settle (ready rejects via the disposed path).
    const all = Promise.allSettled([ready, backend.loadAss("A"), backend.loadAss("B")]);
    backend.dispose();
    const results = await all;
    expect(results).toHaveLength(3);
    expect(results.every((r) => r.status === "rejected")).toBe(true);
    // The worker was never spawned post-dispose: nothing to terminate.
    expect(fake.posted).toHaveLength(0);
  });
});

describe("worker error association (#80)", () => {
  test("out-of-order load responses settle the right promises", async () => {
    const { backend, fake, ready } = readyBackend();
    await handshake(fake, ready);
    const a = backend.loadAss("A");
    const b = backend.loadAss("B");
    // B (requestId 2) succeeds first, A (requestId 1) fails after.
    fake.emit({
      kind: "loaded",
      requestId: 2,
      summary: { resolution: [640, 480], styles: 1, events: 2 },
    });
    fake.emit({ kind: "error", requestId: 1, error: "boom A" });
    await expect(a).rejects.toThrow("boom A");
    await expect(b).resolves.toEqual({ resolution: [640, 480], styles: 1, events: 2 });
  });

  test("render errors clear in-flight and report via onError", async () => {
    const errors: string[] = [];
    const { backend, fake, ready } = readyBackend((m) => errors.push(m));
    await handshake(fake, ready);
    backend.renderFrame(100);
    expect(fake.renders()).toHaveLength(1);
    const inFlight = fake.renders()[0].requestId;
    fake.emit({ kind: "error", requestId: inFlight, error: "render boom" });
    expect(errors).toEqual(["render boom"]);
    // In-flight cleared: the next render sends immediately.
    backend.renderFrame(200);
    expect(fake.renders()).toHaveLength(2);
    expect(fake.renders()[1].timeMs).toBe(200);
  });

  test("errors for unknown request ids only report", async () => {
    const errors: string[] = [];
    const { backend, fake, ready } = readyBackend((m) => errors.push(m));
    await handshake(fake, ready);
    const a = backend.loadAss("A");
    fake.emit({ kind: "error", requestId: 999, error: "stray" });
    expect(errors).toEqual(["stray"]);
    // The real waiter is untouched: it still resolves normally.
    fake.emit({
      kind: "loaded",
      requestId: 1,
      summary: { resolution: [1, 1], styles: 0, events: 0 },
    });
    await expect(a).resolves.toBeDefined();
  });

  test("worker-level failure rejects init and pending loads", async () => {
    const errors: string[] = [];
    const { backend, fake, ready } = readyBackend((m) => errors.push(m));
    // Wait until init() has spawned the worker (wasm init is async).
    for (let i = 0; i < 200 && fake.posted.length === 0; i++) {
      await new Promise((r) => setTimeout(r, 5));
    }
    expect(fake.posted).toHaveLength(1);
    const load = backend.loadAss("A");
    // Attach handlers before the fatal emit: both promises reject
    // from one message, and an await between them would leave the
    // second rejection momentarily unhandled.
    const readyMsg = ready.then(
      () => "resolved",
      (e) => (e as Error).message,
    );
    const loadMsg = load.then(
      () => "resolved",
      (e) => (e as Error).message,
    );
    fake.emit({ kind: "fatal", error: "wasm exploded" });
    expect(await readyMsg).toBe("wasm exploded");
    expect(await loadMsg).toBe("wasm exploded");
    expect(errors).toContain("wasm exploded");
  });
});

describe("render coalescing (#81)", () => {
  test("rapid renders collapse to first + newest", async () => {
    const { backend, fake, ready } = readyBackend();
    await handshake(fake, ready);
    backend.renderFrame(100);
    backend.renderFrame(200);
    backend.renderFrame(300);
    // Only the first render is sent; the rest collapse into one pending.
    expect(fake.renders().map((r) => r.timeMs)).toEqual([100]);
    const first = fake.renders()[0].requestId;
    fake.emit({ kind: "frame", requestId: first, w: 2, h: 2, bytes: new ArrayBuffer(16) });
    // Render 2 was superseded: only 3 follows, with no queue growth.
    expect(fake.renders().map((r) => r.timeMs)).toEqual([100, 300]);
  });

  test("stale frames do not trigger extra renders", async () => {
    const { backend, fake, ready } = readyBackend();
    await handshake(fake, ready);
    backend.renderFrame(100);
    fake.emit({ kind: "frame", requestId: 4242, w: 2, h: 2, bytes: new ArrayBuffer(16) });
    expect(fake.renders()).toHaveLength(1);
  });
});

describe("font transfer (#82)", () => {
  test("subarray views send exact bytes without detaching the pool", async () => {
    const { backend, fake, ready } = readyBackend();
    await handshake(fake, ready);
    const pool = new ArrayBuffer(64);
    new Uint8Array(pool).fill(7);
    const view = new Uint8Array(pool, 8, 16);
    backend.loadFont("f", view);
    expect(pool.byteLength).toBe(64); // not detached
    const sent = fake.posted.filter((p) => p.message.kind === "loadFont");
    expect(sent).toHaveLength(1);
    const data = sent[0].message.data as ArrayBuffer;
    expect(data.byteLength).toBe(16);
    expect(new Uint8Array(data)[0]).toBe(7);
  });
});
