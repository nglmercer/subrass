// Demo driver. Rendering runs in a Web Worker by default; if workers are
// unavailable it falls back to rendering on the main thread.
import init from "../pkg/subrass.js";
import {
    setCanvas,
    createRenderer,
    setVideoSize,
    renderFrame,
    getSummary,
    loadFont,
} from "./renderer.js";
import {
    initPlayer,
    loadVideoUrl,
    isVideoMode,
    togglePlay as togglePlayer,
    stop as stopPlayer,
    seek as seekPlayer,
    getCurrentTimeMs,
    getDurationMs,
    isPaused,
} from "./player.js";

const $ = (id) => document.getElementById(id);
const video = $("video");
const canvas = $("subtitleCanvas");
const ctx = canvas.getContext("2d");
const playPauseBtn = $("playPauseBtn");
const stopBtn = $("stopBtn");
const seekBar = $("seekBar");
const error = $("error");

// ---------------------------------------------------------------------------
// Render backends
// ---------------------------------------------------------------------------

// Main-thread backend (fallback)
const directBackend = {
    kind: "main-thread",
    init: () => init(),
    setCanvas: (c) => setCanvas(c),
    loadAss: async (content) => {
        createRenderer(content, canvas);
        return getSummary();
    },
    setVideoSize: (w, h) => setVideoSize(w, h),
    loadFont: (name, data) => loadFont(name, data),
    renderFrame: (timeMs) => renderFrame(timeMs),
};

// Worker backend: the worker owns the renderer and posts RGBA frames back
function createWorkerBackend() {
    const worker = new Worker(new URL("./worker.js", import.meta.url), {
        type: "module",
    });
    let readyResolve;
    const ready = new Promise((resolve) => (readyResolve = resolve));
    let loadedResolve = null;

    worker.onmessage = (e) => {
        const msg = e.data;
        switch (msg.type) {
            case "ready":
                readyResolve();
                break;
            case "loaded":
                if (loadedResolve) loadedResolve(msg.summary);
                break;
            case "frame": {
                if (canvas.width !== msg.w || canvas.height !== msg.h) {
                    canvas.width = msg.w;
                    canvas.height = msg.h;
                }
                const pixels = new Uint8ClampedArray(msg.buffer);
                ctx.putImageData(new ImageData(pixels, msg.w, msg.h), 0, 0);
                break;
            }
            case "error":
                showError("Worker error: " + msg.message);
                break;
        }
    };
    worker.onerror = (e) => showError("Worker failed: " + e.message);
    worker.postMessage({ type: "init" });

    return {
        kind: "worker",
        init: () => ready,
        setCanvas: () => {},
        loadAss: (content) =>
            new Promise((resolve) => {
                loadedResolve = resolve;
                worker.postMessage({ type: "load", content });
            }),
        setVideoSize: (w, h) => worker.postMessage({ type: "resize", w, h }),
        loadFont: (name, data) =>
            worker.postMessage({ type: "font", name, data: data.buffer }, [data.buffer]),
        renderFrame: (timeMs) => worker.postMessage({ type: "render", timeMs }),
    };
}

let backend;
try {
    backend = createWorkerBackend();
} catch (e) {
    console.warn("Falling back to main-thread rendering:", e);
    backend = directBackend;
}

// ---------------------------------------------------------------------------
// UI wiring
// ---------------------------------------------------------------------------

let loaded = false;

async function main() {
    await backend.init();
    backend.setCanvas(canvas);

    initPlayer(video, {
        onFrame(timeMs) {
            if (endedCheck(timeMs)) return;
            syncCanvasSize();
            if (!isVideoMode()) clearCanvas();
            backend.renderFrame(timeMs);
            updateUI(timeMs);
        },
    });

    $("videoInput").addEventListener("change", (e) => {
        const file = e.target.files[0];
        if (!file) return;
        $("videoName").textContent = file.name;
        loadVideoUrl(URL.createObjectURL(file));
        enableControls();
    });

    $("assInput").addEventListener("change", async (e) => {
        const file = e.target.files[0];
        if (!file) return;
        $("assName").textContent = file.name;
        try {
            await loadAss(await file.text());
        } catch (err) {
            showError("Failed to load ASS file: " + err.message);
        }
    });

    $("fontInput").addEventListener("change", async (e) => {
        const files = e.target.files;
        if (!files.length || !loaded) return;
        const fontList = $("fontList");
        fontList.innerHTML = "Built-in: DejaVu Sans<br>";
        for (const file of files) {
            try {
                const data = new Uint8Array(await file.arrayBuffer());
                backend.loadFont(file.name.replace(/\.[^.]+$/, ""), data);
                fontList.innerHTML += file.name + "<br>";
            } catch (err) {
                console.error("Failed to load font:", file.name, err);
            }
        }
    });

    playPauseBtn.addEventListener("click", () => {
        togglePlayer();
        playPauseBtn.textContent = isPaused() ? "Play" : "Pause";
    });

    stopBtn.addEventListener("click", () => {
        stopPlayer();
        playPauseBtn.textContent = "Play";
        syncCanvasSize();
        if (!isVideoMode()) clearCanvas();
        backend.renderFrame(0);
        updateUI(0);
    });

    seekBar.addEventListener("input", () => {
        seekPlayer(seekBar.value);
        syncCanvasSize();
        if (!isVideoMode()) clearCanvas();
        backend.renderFrame(getCurrentTimeMs());
        updateUI(getCurrentTimeMs());
    });

    video.addEventListener("loadedmetadata", () => {
        syncCanvasSize();
        backend.setVideoSize(video.videoWidth, video.videoHeight);
        backend.renderFrame(getCurrentTimeMs());
    });

    video.addEventListener("timeupdate", () => {
        if (isVideoMode() && !isPaused()) {
            updateUI(getCurrentTimeMs());
        }
    });

    video.addEventListener("ended", () => {
        playPauseBtn.textContent = "Play";
        if ($("loopCheck").checked) {
            video.currentTime = 0;
            video.play();
            playPauseBtn.textContent = "Pause";
        }
    });

    window.addEventListener("resize", () => {
        syncCanvasSize();
        if (isVideoMode()) {
            backend.setVideoSize(video.videoWidth, video.videoHeight);
        }
        backend.renderFrame(getCurrentTimeMs());
    });

    enableControls();
    console.log("Render backend:", backend.kind);

    try {
        const resp = await fetch("test.ass");
        if (resp.ok) {
            await loadAss(await resp.text());
        }
    } catch (e) {
        console.log("Could not auto-load demo ASS file:", e.message);
    }
}

function endedCheck(timeMs) {
    const durMs = getDurationMs();
    if (timeMs < durMs) return false;
    if ($("loopCheck").checked) {
        seekPlayer(0);
        if (!isVideoMode()) {
            syncCanvasSize();
            clearCanvas();
            backend.renderFrame(0);
            updateUI(0);
        }
        return true;
    }
    stopPlayer();
    playPauseBtn.textContent = "Play";
    return true;
}

function syncCanvasSize() {
    let w, h;
    if (video.videoWidth) {
        w = video.videoWidth;
        h = video.videoHeight;
    } else {
        w = 1920;
        h = 1080;
    }
    if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
        backend.setVideoSize(w, h);
    }
}

function clearCanvas() {
    ctx.fillStyle = "#1a1a2e";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
}

async function loadAss(content) {
    const summary = await backend.loadAss(content);
    loaded = true;
    syncCanvasSize();
    if (video.videoWidth) {
        backend.setVideoSize(video.videoWidth, video.videoHeight);
    }
    if (!isVideoMode()) clearCanvas();
    backend.renderFrame(getCurrentTimeMs());
    updateSummary(summary);
    updateUI(getCurrentTimeMs());
}

function updateUI(timeMs) {
    $("timeDisplay").textContent = formatTime(timeMs);
    const durMs = getDurationMs();
    if (!seekBar.matches(":active")) {
        seekBar.value = (timeMs / durMs) * 1000;
    }
}

function updateSummary(s) {
    if (!s) return;
    $("summaryRes").textContent = `${s.resolution[0]}x${s.resolution[1]}`;
    $("summaryStyles").textContent = s.styles;
    $("summaryEvents").textContent = s.events;
}

function enableControls() {
    playPauseBtn.disabled = false;
    stopBtn.disabled = false;
    seekBar.disabled = false;
}

function formatTime(ms) {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 3600)}:${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}.${String(Math.floor(ms % 1000)).padStart(3, "0")}`;
}

function showError(msg) {
    error.textContent = msg;
    error.style.display = "block";
    setTimeout(() => {
        error.style.display = "none";
    }, 5000);
}

main();
