// Central demo debug logging. Disabled by default; enable with
// `?debug` in the URL or `localStorage["subrass-debug"] = "1"`.
function debugEnabled(): boolean {
  try {
    if (typeof location !== "undefined" && /[?&]debug\b/.test(location.search)) return true;
    if (typeof localStorage !== "undefined" && localStorage.getItem("subrass-debug") === "1") {
      return true;
    }
  } catch {
    // Storage/location unavailable (e.g. opaque origin): stay quiet.
  }
  return false;
}

const ENABLED = debugEnabled();

/** Namespaced debug logger; a no-op unless debugging is enabled. */
export function dbg(tag: string): (...args: unknown[]) => void {
  if (!ENABLED) return () => {};
  return (...args: unknown[]) => console.log(`[subrass:demo:${tag}]`, ...args);
}
