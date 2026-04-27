const DEFAULT_DURATION_MS = 600000;

let video = null;
let animFrameId = null;
let animating = false;
let onFrame = null;
let mode = "virtual";

let virtualTime = 0;
let virtualPlaying = false;
let baseWallTime = 0;
let baseVirtualTime = 0;

function cancelAnim() {
    if (animFrameId) {
        cancelAnimationFrame(animFrameId);
        animFrameId = null;
    }
}

export function initPlayer(videoEl, { onFrame: frameCb }) {
    video = videoEl;
    onFrame = frameCb;
}

export function isVideoMode() {
    return mode === "video";
}

export function getDurationMs() {
    if (mode === "video" && video.duration) return video.duration * 1000;
    return DEFAULT_DURATION_MS;
}

export function getCurrentTimeMs() {
    if (mode === "video") return video.currentTime * 1000;
    if (virtualPlaying) {
        return baseVirtualTime + (performance.now() - baseWallTime);
    }
    return virtualTime;
}

export function loadVideoUrl(url) {
    mode = "video";
    virtualPlaying = false;
    video.src = url;
    video.load();
}

function animate() {
    if (!animating) return;
    if (onFrame) onFrame(getCurrentTimeMs());
    animFrameId = requestAnimationFrame(animate);
}

export function play() {
    if (mode === "video") {
        if (!video.paused) return;
        video.play();
    } else {
        if (virtualPlaying) return;
        virtualPlaying = true;
        baseWallTime = performance.now();
        baseVirtualTime = virtualTime;
    }
    animating = true;
    animate();
}

export function pause() {
    if (mode === "video") {
        if (video.paused) return;
        video.pause();
    } else {
        if (!virtualPlaying) return;
        virtualPlaying = false;
        virtualTime = getCurrentTimeMs();
    }
    animating = false;
    cancelAnim();
}

export function togglePlay() {
    if (mode === "video" ? video.paused : !virtualPlaying) {
        play();
    } else {
        pause();
    }
}

export function stop() {
    if (mode === "video") {
        video.pause();
        video.currentTime = 0;
    } else {
        virtualPlaying = false;
        virtualTime = 0;
        baseVirtualTime = 0;
    }
    animating = false;
    cancelAnim();
}

export function seek(fraction) {
    const timeMs = (fraction / 1000) * getDurationMs();
    if (mode === "video") {
        video.currentTime = timeMs / 1000;
    } else {
        virtualTime = timeMs;
        baseVirtualTime = virtualTime;
        baseWallTime = performance.now();
    }
}

export function isPaused() {
    return mode === "video" ? video.paused : !virtualPlaying;
}
