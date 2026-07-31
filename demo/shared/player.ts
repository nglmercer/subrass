// Playback clock for the demo. Drives rendering from either a real <video>
// element or a virtual timeline (requestAnimationFrame + performance.now)
// when no video is loaded, so subtitles can be previewed on their own.

const DEFAULT_VIRTUAL_DURATION_MS = 60_000;

export interface PlayerOptions {
  /** Called on every animation frame while playing, and once after seeks. */
  onFrame: (timeMs: number) => void;
}

export class Player {
  private video: HTMLVideoElement;
  private onFrame: (timeMs: number) => void;

  private mode: "video" | "virtual" = "virtual";
  private animFrameId: number | null = null;

  private virtualDurationMs = DEFAULT_VIRTUAL_DURATION_MS;
  private virtualTime = 0;
  private virtualPlaying = false;
  private baseWallTime = 0;
  private baseVirtualTime = 0;

  constructor(video: HTMLVideoElement, options: PlayerOptions) {
    this.video = video;
    this.onFrame = options.onFrame;
  }

  get videoMode(): boolean {
    return this.mode === "video";
  }

  get paused(): boolean {
    return this.mode === "video" ? this.video.paused : !this.virtualPlaying;
  }

  get durationMs(): number {
    if (this.mode === "video" && Number.isFinite(this.video.duration)) {
      return this.video.duration * 1000;
    }
    return this.virtualDurationMs;
  }

  /** Length of the virtual timeline; typically the last subtitle end + slack. */
  setVirtualDuration(ms: number): void {
    this.virtualDurationMs = Math.max(1000, ms);
    if (this.virtualTime > this.virtualDurationMs) {
      this.seekMs(this.virtualDurationMs);
    }
  }

  currentTimeMs(): number {
    if (this.mode === "video") return this.video.currentTime * 1000;
    if (this.virtualPlaying) {
      return this.baseVirtualTime + (performance.now() - this.baseWallTime);
    }
    return this.virtualTime;
  }

  loadVideoUrl(url: string): void {
    this.mode = "video";
    this.virtualPlaying = false;
    this.stopAnimation();
    this.video.src = url;
    this.video.load();
  }

  play(): void {
    if (this.mode === "video") {
      void this.video.play();
    } else if (!this.virtualPlaying) {
      this.virtualPlaying = true;
      this.baseWallTime = performance.now();
      this.baseVirtualTime = this.virtualTime;
    }
    this.startAnimation();
  }

  pause(): void {
    if (this.mode === "video") {
      this.video.pause();
    } else if (this.virtualPlaying) {
      this.virtualPlaying = false;
      this.virtualTime = this.currentTimeMs();
    }
    this.stopAnimation();
  }

  togglePlay(): void {
    if (this.paused) this.play();
    else this.pause();
  }

  stop(): void {
    if (this.mode === "video") {
      this.video.pause();
      this.video.currentTime = 0;
    } else {
      this.virtualPlaying = false;
      this.virtualTime = 0;
      this.baseVirtualTime = 0;
    }
    this.stopAnimation();
  }

  /** Seek to a fraction of the duration in [0, 1000] (range input units). */
  seek(fraction: number): void {
    this.seekMs((fraction / 1000) * this.durationMs);
  }

  seekMs(timeMs: number): void {
    if (this.mode === "video") {
      this.video.currentTime = timeMs / 1000;
    } else {
      this.virtualTime = Math.max(0, Math.min(timeMs, this.virtualDurationMs));
      this.baseVirtualTime = this.virtualTime;
      this.baseWallTime = performance.now();
    }
    this.onFrame(this.currentTimeMs());
  }

  private startAnimation(): void {
    if (this.animFrameId !== null) return;
    const tick = (): void => {
      this.animFrameId = requestAnimationFrame(tick);
      this.onFrame(this.currentTimeMs());
    };
    this.animFrameId = requestAnimationFrame(tick);
  }

  private stopAnimation(): void {
    if (this.animFrameId !== null) {
      cancelAnimationFrame(this.animFrameId);
      this.animFrameId = null;
    }
  }
}
