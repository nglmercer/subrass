// Worker example entry point: identical UI to the basic example, but the
// renderer runs in a Web Worker (see render-worker.ts for the worker side
// and shared/worker-backend.ts for the main-thread proxy).
import { startDemo } from "../shared/app.ts";
import { WorkerBackend } from "../shared/worker-backend.ts";
import { showError } from "../shared/ui.ts";
import { dbg } from "../shared/debug.ts";

const log = dbg("worker");

const workerUrl = new URL("./render-worker.ts", import.meta.url);
log("entry", {
  href: location.href,
  importMetaUrl: import.meta.url,
  workerUrl: workerUrl.href,
  expectedPkg: new URL("../../pkg/subrass.js", import.meta.url).href,
  note:
    "AssDoc runs on the main thread; WorkerBackend.init() initializes the " +
    "WASM module on the main thread AND the worker initializes its own copy.",
});

const backend = new WorkerBackend(workerUrl, {
  onError: (message) => {
    log("backend onError", message);
    showError(message);
  },
});

startDemo(backend).catch((err) => {
  log("startDemo rejected", err);
  showError(`Demo failed to start: ${(err as Error).message}`);
});
