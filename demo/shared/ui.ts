// DOM helpers shared by both demo pages: element lookup, the summary
// panel, the live "active events" list, and the error toast.
import type { AssDoc } from "../../pkg/subrass.js";
import type { AssEvent, SubtitleSummary } from "./types.ts";
import { formatAssTime, plainText } from "./ass.ts";

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

  constructor(el: HTMLElement, getDoc: () => AssDoc | null) {
    this.el = el;
    this.getDoc = getDoc;
  }

  update(timeMs: number): void {
    const doc = this.getDoc();
    if (!doc) return;
    const events = doc.get_events_at_time(timeMs) as AssEvent[];
    if (events.length === 0) {
      this.el.innerHTML = '<div class="placeholder">No active events</div>';
      return;
    }
    this.el.innerHTML = events
      .map((e) => {
        const range = `${formatAssTime(e.start)}–${formatAssTime(e.end)}`;
        const text = plainText(e) || "(drawing/effect)";
        return `<div class="event-item"><span class="event-time">${range}</span>` +
          `<span class="event-style">${escapeHtml(e.style)}</span>` +
          `<span class="event-text">${escapeHtml(text)}</span></div>`;
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

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!,
  );
}
