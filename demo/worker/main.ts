// Worker example entry point: identical UI to the basic example, but the
// renderer runs in a Web Worker (see render-worker.ts for the worker side
// and shared/worker-backend.ts for the main-thread proxy).
import { startDemo } from "../shared/app.ts";
import { WorkerBackend } from "../shared/worker-backend.ts";
import { showError } from "../shared/ui.ts";

const DEBUG = true;
function dbg(...args: unknown[]): void {
  if (DEBUG) console.log("[subrass:demo:worker]", ...args);
}

const workerUrl = new URL("./render-worker.ts", import.meta.url);
dbg("entry", {
  href: location.href,
  importMetaUrl: import.meta.url,
  workerUrl: workerUrl.href,
  expectedPkg: new URL("../../pkg/subrass.js", import.meta.url).href,
  note:
    "AssDoc is used on the main thread in app.ts; WorkerBackend.init only " +
    "initializes WASM inside the worker. That mismatch causes: " +
    'TypeError: can\'t access property "__wbindgen_malloc", wasm is undefined',
});

const backend = new WorkerBackend(workerUrl, {
  onError: (message) => {
    dbg("backend onError", message);
    showError(message);
  },
});

startDemo(backend).catch((err) => {
  dbg("startDemo rejected", err);
  showError(`Demo failed to start: ${(err as Error).message}`);
});
