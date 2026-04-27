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
    togglePlay as togglePlayer,
    stop as stopPlayer,
    seek as seekPlayer,
    getCurrentTimeMs,
} from "./player.js";

const $ = (id) => document.getElementById(id);
const video = $("video");
const canvas = $("subtitleCanvas");
const playPauseBtn = $("playPauseBtn");
const stopBtn = $("stopBtn");
const seekBar = $("seekBar");
const error = $("error");
const eventList = $("eventList");

async function main() {
    await init();
    setCanvas(canvas);

    initPlayer(video, {
        onFrame(timeMs) {
            syncCanvasSize();
            renderFrame(timeMs);
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
        playPauseBtn.textContent = video.paused ? "Play" : "Pause";
    });

    stopBtn.addEventListener("click", () => {
        stopPlayer();
        playPauseBtn.textContent = "Play";
        syncCanvasSize();
        renderFrame(getCurrentTimeMs());
    });

    seekBar.addEventListener("input", () => {
        seekPlayer(seekBar.value);
        syncCanvasSize();
        renderFrame(getCurrentTimeMs());
    });

    video.addEventListener("loadedmetadata", () => {
        syncCanvasSize();
        setVideoSize(video.videoWidth, video.videoHeight);
        renderFrame(getCurrentTimeMs());
    });

    video.addEventListener("timeupdate", () => {
        const ms = getCurrentTimeMs();
        $("timeDisplay").textContent = formatTime(ms);
        if (!seekBar.matches(":active")) {
            seekBar.value = (ms / 1000 / video.duration) * 1000;
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
        if (video.videoWidth) {
            syncCanvasSize();
            setVideoSize(video.videoWidth, video.videoHeight);
            renderFrame(getCurrentTimeMs());
        }
    });

    $("videoName").textContent = "BigBuckBunny.mp4";
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

function syncCanvasSize() {
    if (!video.videoWidth) return;
    if (
        canvas.width !== video.videoWidth ||
        canvas.height !== video.videoHeight
    ) {
        canvas.width = video.videoWidth;
        canvas.height = video.videoHeight;
        setVideoSize(video.videoWidth, video.videoHeight);
    }
}

async function loadAss(content) {
    createRenderer(content, canvas);
    if (video.videoWidth) {
        setVideoSize(video.videoWidth, video.videoHeight);
    }
    syncCanvasSize();
    renderFrame(getCurrentTimeMs());
    updateSummary();
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
