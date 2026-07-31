// Small helpers for working with parsed ASS data in the demo UI.
import type { AssEvent, AssTime } from "./types.ts";

/** Convert a parsed ASS time to milliseconds. */
export function timeToMs(t: AssTime): number {
  return ((t.hours * 3600 + t.minutes * 60 + t.seconds) * 100 + t.centiseconds) * 10;
}

/** Format milliseconds as H:MM:SS.mmm for the playback clock. */
export function formatClock(ms: number): string {
  const totalSec = Math.floor(ms / 1000);
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  const millis = Math.floor(ms % 1000);
  return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(millis).padStart(3, "0")}`;
}

/** Format a parsed ASS time as M:SS.CC (compact, for the event list). */
export function formatAssTime(t: AssTime): string {
  const totalSec = t.hours * 3600 + t.minutes * 60 + t.seconds;
  return `${Math.floor(totalSec / 60)}:${String(totalSec % 60).padStart(2, "0")}.${String(t.centiseconds).padStart(2, "0")}`;
}

/** Strip ASS override blocks ({\...}) and convert hard breaks to spaces. */
export function plainText(event: AssEvent): string {
  return event.text.replace(/\{[^}]*\}/g, "").replace(/\\[Nnh]/g, " ").trim();
}

/** End time (ms) of the last event, or 0 for an empty file. */
export function lastEventEndMs(events: AssEvent[]): number {
  let end = 0;
  for (const e of events) {
    end = Math.max(end, timeToMs(e.end));
  }
  return end;
}
