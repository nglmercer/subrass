// The worker side of the worker example. Owns the WASM SubtitleRenderer
// and answers render requests with RGBA frames, transferring the pixel
// buffer instead of copying it.
//
// Protocol (mirrored by shared/worker-backend.ts):
//   main  -> worker: init | load | font | resize | render{timeMs, seq}
//   worker -> main:  ready | loaded{summary} | frame{seq, w, h, buffer} | error
import init, { SubtitleRenderer } from "../../pkg/subrass.js";

type MainMessage =
  | { type: "init" }
  | { type: "load"; content: string }
  | { type: "font"; name: string; data: ArrayBuffer }
  | { type: "resize"; w: number; h: number }
  | { type: "render"; timeMs: number; seq: number };

// Minimal structural typing for the worker scope, so this file does not
// depend on the DOM/WebWorker lib split.
const scope = self as unknown as {
  onmessage: ((e: MessageEvent<MainMessage>) => void) | null;
  postMessage(message: unknown, transfer?: Transferable[]): void;
};

let renderer: SubtitleRenderer | null = null;
let frameW = 1920;
let frameH = 1080;

scope.onmessage = async (e) => {
  const msg = e.data;
  try {
    switch (msg.type) {
      case "init": {
        await init();
        scope.postMessage({ type: "ready" });
        break;
      }
      case "load": {
        renderer?.free();
        renderer = new SubtitleRenderer(msg.content);
        const res = renderer.get_play_resolution();
        frameW = res[0] || 1920;
        frameH = res[1] || 1080;
        renderer.set_video_size(frameW, frameH);
        scope.postMessage({
          type: "loaded",
          summary: {
            resolution: [frameW, frameH],
            styles: renderer.get_style_count(),
            events: renderer.get_event_count(),
          },
        });
        break;
      }
      case "font": {
        renderer?.load_font(msg.name, new Uint8Array(msg.data));
        break;
      }
      case "resize": {
        frameW = msg.w;
        frameH = msg.h;
        renderer?.set_video_size(msg.w, msg.h);
        break;
      }
      case "render": {
        if (!renderer) break;
        renderer.render_frame(msg.timeMs);
        const size = renderer.get_frame_size();
        const bytes = renderer.get_frame_data();
        scope.postMessage(
          {
            type: "frame",
            seq: msg.seq,
            w: size[0] || frameW,
            h: size[1] || frameH,
            buffer: bytes.buffer,
          },
          [bytes.buffer],
        );
        break;
      }
    }
  } catch (err) {
    scope.postMessage({
      type: "error",
      message: String((err as Error | undefined)?.message ?? err),
    });
  }
};
