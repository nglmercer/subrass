import init from "../pkg/subrass.js";
import {
    setCanvas,
    createRenderer,
    setVideoSize,
    renderFrame,
    getSummary,
    loadFont,
    getRenderer,
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

async function main() {
    await init();
    setCanvas(canvas);

    initPlayer(video, {
        onFrame(timeMs) {
            if (endedCheck(timeMs)) return;
            syncCanvasSize();
            if (!isVideoMode()) clearCanvas();
            renderFrame(timeMs);
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
        if (!files.length || !getRenderer()) return;
        const fontList = $("fontList");
        fontList.innerHTML = "Built-in: DejaVu Sans<br>";
        for (const file of files) {
            try {
                const data = new Uint8Array(await file.arrayBuffer());
                loadFont(file.name.replace(/\.[^.]+$/, ""), data);
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
        renderFrame(0);
        updateUI(0);
    });

    seekBar.addEventListener("input", () => {
        seekPlayer(seekBar.value);
        syncCanvasSize();
        if (!isVideoMode()) clearCanvas();
        renderFrame(getCurrentTimeMs());
        updateUI(getCurrentTimeMs());
    });

    video.addEventListener("loadedmetadata", () => {
        syncCanvasSize();
        setVideoSize(video.videoWidth, video.videoHeight);
        renderFrame(getCurrentTimeMs());
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
            setVideoSize(video.videoWidth, video.videoHeight);
        }
        renderFrame(getCurrentTimeMs());
    });

    enableControls();

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
            renderFrame(0);
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
        setVideoSize(w, h);
    }
}

function clearCanvas() {
    ctx.fillStyle = "#1a1a2e";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
}

async function loadAss(content) {
    createRenderer(content, canvas);
    syncCanvasSize();
    if (video.videoWidth) {
        setVideoSize(video.videoWidth, video.videoHeight);
    }
    if (!isVideoMode()) clearCanvas();
    renderFrame(getCurrentTimeMs());
    updateSummary();
    updateUI(getCurrentTimeMs());
}

function updateUI(timeMs) {
    $("timeDisplay").textContent = formatTime(timeMs);
    const durMs = getDurationMs();
    if (!seekBar.matches(":active")) {
        seekBar.value = (timeMs / durMs) * 1000;
    }
}

function updateSummary() {
    const s = getSummary();
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
