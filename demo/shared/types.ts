// Shared type definitions for the subrass demo.
//
// These mirror the serde shapes produced by the Rust crate (see
// src/types/*.rs) and the rendering backend contract shared by the
// main-thread and Web Worker examples.

/** ASS time as serialized by the parser: H:MM:SS.CC components. */
export interface AssTime {
  hours: number;
  minutes: number;
  seconds: number;
  centiseconds: number;
}

/** A dialogue/comment event as returned by `AssDoc.get_events_at_time`. */
export interface AssEvent {
  layer: number;
  start: AssTime;
  end: AssTime;
  style: string;
  name: string;
  margin_l: number;
  margin_r: number;
  margin_v: number;
  effect: string;
  text: string;
}

/** Subset of `[Script Info]` exposed by `AssDoc.get_script_info`. */
export interface ScriptInfo {
  title: string | null;
  play_res_x: number;
  play_res_y: number;
  wrap_style: number;
}

/** Metadata returned when a subtitle file is loaded into a backend. */
export interface SubtitleSummary {
  resolution: [number, number];
  styles: number;
  events: number;
}

/**
 * A rendering backend draws subtitle frames for a given playback time.
 *
 * Two implementations exist: `DirectBackend` renders on the main thread,
 * and `WorkerBackend` proxies the same operations to a Web Worker that
 * owns the WASM renderer and posts RGBA frames back.
 */
export interface RenderBackend {
  /** Human-readable backend name, shown in the UI badge. */
  readonly kind: string;
  /** Initialize the WASM module. Resolves when ready to load subtitles. */
  init(): Promise<void>;
  /** Canvas that receives frames (drawn directly, or via worker messages). */
  setFrameTarget(canvas: HTMLCanvasElement): void;
  /** Parse and load ASS/SSA content; resolves with file metadata. */
  loadAss(content: string): Promise<SubtitleSummary>;
  /** Set the render resolution (usually the video or script resolution). */
  resize(width: number, height: number): void;
  /** Register an additional font (TTF/OTF bytes) with the renderer. */
  loadFont(name: string, data: Uint8Array): void;
  /** Render the frame for the given playback time. */
  renderFrame(timeMs: number): void;
  /** Release the backend (terminates the worker, if any). */
  dispose(): void;
}
