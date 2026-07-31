// Worker example entry point: identical UI to the basic example, but the
// renderer runs in a Web Worker (see render-worker.ts for the worker side
// and shared/worker-backend.ts for the main-thread proxy).
import { startDemo } from "../shared/app.ts";
import { WorkerBackend } from "../shared/worker-backend.ts";
import { showError } from "../shared/ui.ts";

const backend = new WorkerBackend(new URL("./render-worker.ts", import.meta.url), {
  onError: showError,
});

startDemo(backend).catch((err) => {
  showError(`Demo failed to start: ${(err as Error).message}`);
});
