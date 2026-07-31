// Basic example entry point: render subtitles on the main thread.
// This is the minimal integration — one backend, one canvas, no workers.
import { startDemo } from "../shared/app.ts";
import { DirectBackend } from "../shared/direct-backend.ts";
import { showError } from "../shared/ui.ts";

const DEBUG = true;
function dbg(...args: unknown[]): void {
  if (DEBUG) console.log("[subrass:demo:basic]", ...args);
}

dbg("entry", {
  href: location.href,
  importMetaUrl: import.meta.url,
  expectedPkg: new URL("../../pkg/subrass.js", import.meta.url).href,
});

startDemo(new DirectBackend()).catch((err) => {
  dbg("startDemo rejected", err);
  showError(`Demo failed to start: ${(err as Error).message}`);
});
