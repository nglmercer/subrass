import { SubtitleRenderer } from "../pkg/subrass.js";

let renderer = null;
let canvas = null;

export function setCanvas(c) {
    canvas = c;
}

export function createRenderer(content, c) {
    canvas = c;
    renderer = new SubtitleRenderer(content);
    renderer.set_canvas(canvas);
    return renderer;
}

export function getRenderer() {
    return renderer;
}

export function setVideoSize(w, h) {
    if (renderer && w && h) {
        renderer.set_video_size(w, h);
    }
}

export function renderFrame(timeMs) {
    if (!renderer || !canvas) return;
    renderer.set_canvas(canvas);
    renderer.render_frame(timeMs);
}

export function getSummary() {
    if (!renderer) return null;
    return {
        resolution: renderer.get_play_resolution(),
        styles: renderer.get_style_count(),
        events: renderer.get_event_count(),
    };
}

export function loadFont(name, data) {
    if (renderer) {
        renderer.load_font(name, data);
    }
}
