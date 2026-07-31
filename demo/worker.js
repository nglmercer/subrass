// Web Worker that owns the WASM subtitle renderer. The main thread sends
// time ticks and receives RGBA frames back (transferred, not copied).
import init, { SubtitleRenderer } from "../pkg/subrass.js";

let renderer = null;
let frameW = 0;
let frameH = 0;

self.onmessage = async (e) => {
    const msg = e.data;
    try {
        switch (msg.type) {
            case "init": {
                await init();
                self.postMessage({ type: "ready" });
                break;
            }
            case "load": {
                renderer = new SubtitleRenderer(msg.content);
                const res = renderer.get_play_resolution();
                frameW = res[0] || 1920;
                frameH = res[1] || 1080;
                renderer.set_video_size(frameW, frameH);
                self.postMessage({
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
                if (renderer) {
                    renderer.load_font(msg.name, new Uint8Array(msg.data));
                }
                break;
            }
            case "resize": {
                frameW = msg.w;
                frameH = msg.h;
                if (renderer) {
                    renderer.set_video_size(msg.w, msg.h);
                }
                break;
            }
            case "render": {
                if (!renderer) break;
                renderer.render_frame(msg.timeMs);
                const bytes = renderer.get_frame_data();
                self.postMessage(
                    {
                        type: "frame",
                        timeMs: msg.timeMs,
                        w: frameW,
                        h: frameH,
                        buffer: bytes.buffer,
                    },
                    [bytes.buffer],
                );
                break;
            }
        }
    } catch (err) {
        self.postMessage({
            type: "error",
            message: String((err && err.message) || err),
        });
    }
};
