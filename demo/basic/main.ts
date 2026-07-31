// Basic example entry point: render subtitles on the main thread.
// This is the minimal integration — one backend, one canvas, no workers.
import { startDemo } from "../shared/app.ts";
import { DirectBackend } from "../shared/direct-backend.ts";
import { showError } from "../shared/ui.ts";

startDemo(new DirectBackend()).catch((err) => {
  showError(`Demo failed to start: ${(err as Error).message}`);
});
