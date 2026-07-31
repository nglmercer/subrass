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

export interface DemoOptions {
  /** Subtitle file fetched on startup. Defaults to the bundled sample. */
  sampleUrl?: string;
}

export async function startDemo(backend: RenderBackend, options: DemoOptions = {}): Promise<void> {
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
      if (!player.videoMode) paintBackdrop();
      backend.renderFrame(timeMs);
      updateHud(timeMs);
    },
  });

  await backend.init();
  backend.setFrameTarget(canvas);

  // --- Subtitle loading -----------------------------------------------------

  async function loadAss(content: string): Promise<void> {
    doc?.free();
    doc = new AssDoc(content);
    summary = await backend.loadAss(content);
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
    if (!player.videoMode) paintBackdrop();
    backend.renderFrame(0);
    updateHud(0);
  });

  seekBar.addEventListener("input", () => {
    player.seek(Number(seekBar.value));
    syncCanvasSize();
    if (!player.videoMode) paintBackdrop();
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
    const resp = await fetch(options.sampleUrl ?? "/sample.ass");
    if (resp.ok) {
      byId("assName").textContent = "sample.ass";
      await loadAss(await resp.text());
    }
  } catch (err) {
    console.warn("Could not auto-load the sample subtitles:", err);
  }
}
