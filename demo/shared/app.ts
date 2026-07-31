// Shared demo wiring: playback controls, file inputs, canvas sizing and
// the info panels. Each example page creates a RenderBackend (main-thread
// or worker) and hands it to startDemo() — everything else is identical.
import { AssDoc } from "../../pkg/subrass.js";
import type { AssEvent, RenderBackend, ScriptInfo, SubtitleSummary } from "./types.ts";
import { formatClock, lastEventEndMs } from "./ass.ts";
import { Player } from "./player.ts";
import { ActiveEventList, byId, showError, updateSummary } from "./ui.ts";

const DEFAULT_WIDTH = 1920;
const DEFAULT_HEIGHT = 1080;
const END_SLACK_MS = 3000;

const DEBUG = true;
function dbg(...args: unknown[]): void {
  if (DEBUG) console.log("[subrass:demo:app]", ...args);
}

export interface DemoOptions {
  /** Subtitle file fetched on startup. Defaults to the bundled sample. */
  sampleUrl?: string;
}

export async function startDemo(backend: RenderBackend, options: DemoOptions = {}): Promise<void> {
  dbg("startDemo begin", {
    backendKind: backend.kind,
    sampleUrl: options.sampleUrl ?? "/sample.ass",
    href: typeof location !== "undefined" ? location.href : "(no location)",
    importMetaUrl: import.meta.url,
  });

  const video = byId<HTMLVideoElement>("video");
  const canvas = byId<HTMLCanvasElement>("subtitleCanvas");
  const ctx = canvas.getContext("2d")!;
  const playPauseBtn = byId<HTMLButtonElement>("playPauseBtn");
  const stopBtn = byId<HTMLButtonElement>("stopBtn");
  const seekBar = byId<HTMLInputElement>("seekBar");
  const loopCheck = byId<HTMLInputElement>("loopCheck");

  byId("backendBadge").textContent = backend.kind;

  let doc: AssDoc | null = null;
  let summary: SubtitleSummary | null = null;
  let loaded = false;

  const eventList = new ActiveEventList(byId("eventList"), () => doc);
  const player = new Player(video, {
    onFrame(timeMs) {
      if (handleEnd(timeMs)) return;
      syncCanvasSize();
      backend.renderFrame(timeMs);
      updateHud(timeMs);
    },
  });

  dbg("calling backend.init() — this must initialize WASM for the renderer");
  const initStarted = performance.now();
  await backend.init();
  dbg("backend.init() done", {
    ms: +(performance.now() - initStarted).toFixed(1),
    // AssDoc lives on the main thread and needs the module's init() to have
    // run on this thread. WorkerBackend.init() now calls init() for both the
    // worker and the main thread, so AssDoc has its bindings ready.
    note:
      backend.kind === "web-worker"
        ? "worker backend: main-thread pkg/subrass.js initialized in WorkerBackend.init()"
        : "main-thread backend: DirectBackend.init() called init() on this module",
  });

  backend.setFrameTarget(canvas);
  dbg("setFrameTarget done", { canvasW: canvas.width, canvasH: canvas.height });

  // --- Subtitle loading -----------------------------------------------------

  async function loadAss(content: string): Promise<void> {
    dbg("loadAss begin", {
      contentBytes: content.length,
      contentHead: content.slice(0, 40).replace(/\n/g, "\\n"),
    });
    doc?.free();
    try {
      dbg("constructing AssDoc on main thread (requires wasm bindings)");
      doc = new AssDoc(content);
      dbg("AssDoc constructed ok");
    } catch (err) {
      dbg("AssDoc constructor FAILED — classic symptom: wasm is undefined", {
        error: err,
        message: (err as Error)?.message,
        hint:
          "pkg/subrass.js keeps a module-level `wasm` set only by init()/initSync(). " +
          "If backend.init() only ran inside a Worker, the main thread never set it.",
      });
      throw err;
    }
    summary = await backend.loadAss(content);
    dbg("backend.loadAss done", { summary });
    loaded = true;

    const events = doc!.get_dialogue_events() as AssEvent[];
    player.setVirtualDuration(lastEventEndMs(events) + END_SLACK_MS);

    syncCanvasSize();
    if (!player.videoMode) paintBackdrop();
    backend.renderFrame(player.currentTimeMs());
    refreshHud();
  }

  // --- Canvas sizing --------------------------------------------------------

  function syncCanvasSize(): void {
    const w = video.videoWidth || DEFAULT_WIDTH;
    const h = video.videoHeight || DEFAULT_HEIGHT;
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
      backend.resize(w, h);
    }
  }

  function paintBackdrop(): void {
    ctx.fillStyle = "#1a1a2e";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
  }

  // --- HUD (clock, seek bar, panels) -----------------------------------------

  function updateHud(timeMs: number): void {
    byId("timeDisplay").textContent = formatClock(timeMs);
    if (!seekBar.matches(":active")) {
      seekBar.value = String((timeMs / player.durationMs) * 1000);
    }
    eventList.update(timeMs);
  }

  function refreshHud(): void {
    const info = doc ? (doc.get_script_info() as ScriptInfo) : null;
    updateSummary(summary, info?.title ?? null);
    updateHud(player.currentTimeMs());
  }

  function handleEnd(timeMs: number): boolean {
    if (timeMs < player.durationMs) return false;
    if (loopCheck.checked) {
      player.seekMs(0);
      return true;
    }
    player.stop();
    playPauseBtn.textContent = "Play";
    player.seekMs(0);
    return true;
  }

  // --- Controls ---------------------------------------------------------------

  playPauseBtn.addEventListener("click", () => {
    player.togglePlay();
    playPauseBtn.textContent = player.paused ? "Play" : "Pause";
  });

  stopBtn.addEventListener("click", () => {
    player.stop();
    playPauseBtn.textContent = "Play";
    syncCanvasSize();
    backend.renderFrame(0);
    updateHud(0);
  });

  seekBar.addEventListener("input", () => {
    player.seek(Number(seekBar.value));
    syncCanvasSize();
    backend.renderFrame(player.currentTimeMs());
    updateHud(player.currentTimeMs());
  });

  video.addEventListener("loadedmetadata", () => {
    syncCanvasSize();
    backend.renderFrame(player.currentTimeMs());
  });

  video.addEventListener("ended", () => {
    playPauseBtn.textContent = "Play";
  });

  window.addEventListener("resize", () => {
    syncCanvasSize();
    backend.renderFrame(player.currentTimeMs());
  });

  // --- File inputs --------------------------------------------------------------

  byId<HTMLInputElement>("videoInput").addEventListener("change", (e) => {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (!file) return;
    byId("videoName").textContent = file.name;
    player.loadVideoUrl(URL.createObjectURL(file));
  });

  byId<HTMLInputElement>("assInput").addEventListener("change", async (e) => {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (!file) return;
    byId("assName").textContent = file.name;
    try {
      await loadAss(await file.text());
    } catch (err) {
      showError(`Failed to load subtitle file: ${(err as Error).message}`);
    }
  });

  byId<HTMLInputElement>("fontInput").addEventListener("change", async (e) => {
    const files = (e.target as HTMLInputElement).files;
    if (!files?.length || !loaded) return;
    const fontList = byId("fontList");
    fontList.textContent = "Built-in: DejaVu Sans";
    for (const file of files) {
      try {
        const data = new Uint8Array(await file.arrayBuffer());
        backend.loadFont(file.name.replace(/\.[^.]+$/, ""), data);
        fontList.textContent += `\n${file.name}`;
      } catch (err) {
        console.error("Failed to load font:", file.name, err);
      }
    }
  });

  // --- Startup -------------------------------------------------------------------

  playPauseBtn.disabled = false;
  stopBtn.disabled = false;
  seekBar.disabled = false;
  refreshHud();

  try {
    const sampleUrl = options.sampleUrl ?? "/sample.ass";
    dbg("fetching sample", sampleUrl);
    const resp = await fetch(sampleUrl);
    dbg("sample fetch", { ok: resp.ok, status: resp.status, url: resp.url });
    if (resp.ok) {
      byId("assName").textContent = "sample.ass";
      await loadAss(await resp.text());
      dbg("sample loadAss finished successfully");
    } else {
      dbg("sample not loaded (non-OK response)");
    }
  } catch (err) {
    dbg("sample auto-load failed", err);
    console.warn("Could not auto-load the sample subtitles:", err);
  }

  dbg("startDemo complete", { loaded, hasDoc: !!doc, hasSummary: !!summary });
}
