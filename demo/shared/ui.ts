// DOM helpers shared by both demo pages: element lookup, the summary
// panel, the live "active events" list, and the error toast.
import type { AssDoc } from "../../pkg/subrass.js";
import type { AssEvent, SubtitleSummary } from "./types.ts";
import { formatAssTime, plainText, timeToMs } from "./ass.ts";

/** Look up an element by id with the expected type, or throw. */
export function byId<T extends HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) throw new Error(`Missing element #${id}`);
  return el as T;
}

export function updateSummary(summary: SubtitleSummary | null, title: string | null): void {
  byId("summaryRes").textContent = summary ? `${summary.resolution[0]}x${summary.resolution[1]}` : "-";
  byId("summaryStyles").textContent = summary ? String(summary.styles) : "-";
  byId("summaryEvents").textContent = summary ? String(summary.events) : "-";
  byId("summaryTitle").textContent = title || "-";
}

/** Renders the events active at the current playback time. */
export class ActiveEventList {
  private el: HTMLElement;
  private getDoc: () => AssDoc | null;
  private cachedDoc: AssDoc | null = null;
  private events: AssEvent[] = [];
  private signature = "";

  constructor(el: HTMLElement, getDoc: () => AssDoc | null) {
    this.el = el;
    this.getDoc = getDoc;
  }

  update(timeMs: number): void {
    const doc = this.getDoc();
    if (!doc) return;
    if (doc !== this.cachedDoc) {
      this.cachedDoc = doc;
      this.events = doc.get_events() as AssEvent[];
      this.signature = "";
    }
    const events = this.events.map((event, index) => ({ event, index }))
      .filter(({ event: e }) => e.event_type !== "Comment" && timeToMs(e.start) <= timeMs && timeMs < timeToMs(e.end))
      .sort((a, b) => a.event.layer - b.event.layer);
    const signature = events.map(({ index }) => index).join(",") || "empty";
    if (signature === this.signature) return;
    this.signature = signature;
    if (events.length === 0) {
      this.el.innerHTML = '<div class="placeholder">No active events</div>';
      return;
    }
    this.el.innerHTML = events
      .map(({ event: e, index }) => {
        const range = `${formatAssTime(e.start)}–${formatAssTime(e.end)}`;
        const text = plainText(e) || "(drawing/effect)";
        return `<button type="button" class="event-item" data-event-index="${index}"><span class="event-time">${range}</span>` +
          `<span class="event-style">Layer ${e.layer} · ${escapeHtml(e.style)}</span>` +
          `<span class="event-text">${escapeHtml(text.slice(0, 240))}</span></button>`;
      })
      .join("");
  }
}

let errorTimer: number | undefined;

export function showError(msg: string): void {
  const el = byId("error");
  el.textContent = msg;
  el.style.display = "block";
  window.clearTimeout(errorTimer);
  errorTimer = window.setTimeout(() => {
    el.style.display = "none";
  }, 6000);
}

export function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!,
  );
}
