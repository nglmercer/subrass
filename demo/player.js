let video = null;
let animFrameId = null;
let animating = false;
let onFrame = null;

export function initPlayer(videoEl, { onFrame: frameCb }) {
    video = videoEl;
    onFrame = frameCb;
}

export function loadVideoUrl(url) {
    video.src = url;
    video.load();
}

function animate() {
    if (!animating) return;
    if (onFrame) onFrame(video.currentTime * 1000);
    animFrameId = requestAnimationFrame(animate);
}

export function play() {
    if (!video.paused) return;
    video.play();
    animating = true;
    animate();
}

export function pause() {
    if (video.paused) return;
    video.pause();
    animating = false;
    if (animFrameId) {
        cancelAnimationFrame(animFrameId);
        animFrameId = null;
    }
}

export function isPaused() {
    return video.paused;
}

export function togglePlay() {
    if (video.paused) play();
    else pause();
}

export function stop() {
    video.pause();
    video.currentTime = 0;
    animating = false;
    if (animFrameId) {
        cancelAnimationFrame(animFrameId);
        animFrameId = null;
    }
}

export function seek(fraction) {
    video.currentTime = (fraction / 1000) * video.duration;
}

export function getCurrentTimeMs() {
    return video.currentTime * 1000;
}

export function getVideoDimensions() {
    return { w: video.videoWidth, h: video.videoHeight };
}
