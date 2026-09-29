import type { AssDoc } from "../../pkg/subrass.js";
import type { AssEvent, DebugFrame, EventDebug } from "./types.ts";
import { formatAssTime, formatClock, plainText } from "./ass.ts";
import { escapeHtml } from "./ui.ts";

/** Hit-test layout rectangles from front to back; equal layers keep paint order. */
export function hitTest(events: EventDebug[], x: number, y: number): EventDebug | undefined {
  return events.findLast(({ layout_bounds: [l, t, r, b] }) =>
    [l, t, r, b].every(Number.isFinite) && x >= l && x <= r && y >= t && y <= b);
}

/** Convert CSS coordinates independently on both axes (zoom/responsive/video size). */
export function framePoint(clientX: number, clientY: number,
  rect: Pick<DOMRect, "left" | "top" | "width" | "height">, width: number, height: number): [number, number] {
  return [(clientX - rect.left) / rect.width * width, (clientY - rect.top) / rect.height * height];
}

const num = (value: number) => Number.isFinite(value) ? String(Math.round(value * 100) / 100) : "—";
const vec = (values: number[]) => values.map(num).join(" / ");
const rgba = (values: number[]) => `#${values.slice(0, 3).map(v => v.toString(16).padStart(2, "0")).join("")} · α ${num(values[3]! / 255)}`;
const rows = (properties: [string, string][]) => '<dl class="props">' + properties.map(([key, value]) =>
  `<dt>${escapeHtml(key)}</dt><dd>${escapeHtml(value)}</dd>`).join("") + "</dl>";

/** Safe HTML with source properties kept separate from measured per-run state. */
export function describeEvent(event: AssEvent, debug: EventDebug, timeMs: number): string {
  return `<h3 class="inspector-title">${escapeHtml(plainText(event).slice(0, 160) || "Vector drawing")}</h3>` +
    `<p class="inspector-time">Frame ${formatClock(timeMs)} · event #${debug.event_index + 1} · layer ${event.layer}</p>` +
    debug.runs.map((run, index) =>
      `<p class="run-heading">Run ${index + 1}${run.drawing_mode ? ` · drawing p${run.drawing_mode}` : " · text"}</p>` + rows([
        ["Font request", `${run.font_name} / ${num(run.font_size)}`],
        ["Fill RGBA", rgba(run.fill)], ["Outline RGBA", rgba(run.outline_color)],
        ["Scale X/Y", vec(run.scale)], ["Rotation X/Y/Z", vec(run.rotation)],
        ["Shear X/Y", vec(run.shear)], ["Border X/Y", vec(run.border)],
        ["Shadow X/Y", vec(run.shadow)], ["Blur / be", `${num(run.blur)} / ${run.edge_blur}`],
      ])).join("") + (debug.run_count > debug.runs.length ?
      `<p class="inspector-help">Showing ${debug.runs.length} of ${debug.run_count} runs.</p>` : "") +
    '<p class="run-heading">Event geometry &amp; source fields</p>' + rows([
      ["Timing", `${formatAssTime(event.start)} → ${formatAssTime(event.end)}`],
      ["Layer / style", `${event.layer} / ${event.style}`],
      ["Actor", event.name || "—"], ["Effect", event.effect || "—"],
      ["Margins L/R/V", `${event.margin_l} / ${event.margin_r} / ${event.margin_v}`],
      ["Alignment", String(debug.alignment)], ["Layout px", vec(debug.layout_bounds)],
      ["Origin px", vec(debug.origin)],
      ["Collision", debug.collision_eligible ? `Eligible · shift ${num(debug.collision_shift)} px` : "Excluded"],
      ["Fade opacity", num(debug.opacity)],
    ]) +
    '<p class="source-label">ASS source · overrides and text</p>' +
    `<pre>${escapeHtml(event.text.slice(0, 4000))}${event.text.length > 4000 ? "\n… source truncated" : ""}</pre>`;
}

/** Inspection is disabled by default; enabling it never writes to the subtitle canvas. */
export class EventInspector {
  private enabled = false;
  private frame: DebugFrame | null = null;
  private events: AssEvent[] = [];
  private point: [number, number] | null = null;
  private cardIndex: number | null = null;
  private lastDetails = 0;
  private selected: number | null = null;

  constructor(private stage: HTMLElement, private overlay: HTMLElement,
    private tooltip: HTMLElement, private details: HTMLElement, private status: HTMLElement,
    eventList: HTMLElement) {
    stage.addEventListener("pointermove", e => {
      const rect = stage.getBoundingClientRect();
      this.point = framePoint(e.clientX, e.clientY, rect, 1, 1);
      this.cardIndex = null;
      this.refresh(true);
    });
    stage.addEventListener("pointerleave", () => {
      this.point = null;
      this.cardIndex = this.selected; // Keep details available for scrolling/copying.
      this.refresh(true);
    });
    const inspectCard = (e: Event) => {
      const card = (e.target as HTMLElement).closest<HTMLElement>("[data-event-index]");
      if (card) { this.point = null; this.cardIndex = Number(card.dataset.eventIndex); this.refresh(true); }
    };
    for (const type of ["pointerover", "focusin", "click"]) eventList.addEventListener(type, inspectCard);
    eventList.addEventListener("pointerleave", () => {
      if (!eventList.contains(document.activeElement)) { this.cardIndex = this.selected; this.refresh(true); }
    });
    eventList.addEventListener("focusout", e => {
      if (!eventList.contains(e.relatedTarget as Node | null)) { this.cardIndex = this.selected; this.refresh(true); }
    });
  }

  setDocument(doc: AssDoc): void {
    this.events = doc.get_events() as AssEvent[];
    this.frame = null;
    this.cardIndex = null;
    this.refresh(true);
  }

  setEnabled(enabled: boolean): void {
    this.enabled = enabled;
    this.frame = null;
    this.stage.classList.toggle("inspecting", enabled);
    this.status.textContent = enabled ? "Hover an event" : "Off";
    this.refresh(true);
  }

  onFrame = (frame: DebugFrame): void => {
    this.frame = frame;
    this.refresh(false);
  };

  private refresh(force: boolean): void {
    if (!this.enabled || !this.frame) {
      this.overlay.innerHTML = "";
      this.tooltip.hidden = true;
      this.details.innerHTML = `<div class="placeholder">${this.enabled ? "Hover a subtitle or focus an active event to inspect it." : "Enable “Inspect on hover” to see event and run properties."}</div>`;
      this.selected = null;
      return;
    }
    const { width, height, events, timeMs } = this.frame;
    const hovered = this.point ? hitTest(events, this.point[0] * width, this.point[1] * height) : undefined;
    const debug = this.cardIndex !== null ? events.find(e => e.event_index === this.cardIndex) :
      hovered ?? events.find(e => e.event_index === this.selected);
    const event = debug ? this.events[debug.event_index] : undefined;
    const chosen = event && debug ? debug.event_index : null;
    this.overlay.innerHTML = events.slice(0, 64).filter(e => e.layout_bounds.every(Number.isFinite))
      .map(e => {
        const [l, t, r, b] = e.layout_bounds;
        return `<div class="debug-box${e.event_index === chosen ? " selected" : ""}" style="left:${l / width * 100}%;top:${t / height * 100}%;width:${Math.max(0, r - l) / width * 100}%;height:${Math.max(0, b - t) / height * 100}%"><span>#${e.event_index + 1}</span></div>`;
      }).join("");
    this.tooltip.hidden = !(hovered && debug && event && this.point);
    if (hovered && debug && event && this.point) {
      this.tooltip.textContent = `${plainText(event).slice(0, 70) || "Vector drawing"}\nLayer ${event.layer} · ${event.style} · ${debug.run_count} run(s)\n${formatClock(timeMs)} · see Event inspector`;
      // Clamp inside the preview even near edges and at browser zoom.
      const x = this.point[0] * this.stage.clientWidth;
      const y = this.point[1] * this.stage.clientHeight;
      this.tooltip.style.left = `${Math.max(8, Math.min(x + 14, this.stage.clientWidth - this.tooltip.offsetWidth - 8))}px`;
      this.tooltip.style.top = `${Math.max(8, Math.min(y + 18, this.stage.clientHeight - this.tooltip.offsetHeight - 8))}px`;
    }
    if (force || chosen !== this.selected || performance.now() - this.lastDetails > 120) {
      const scroll = chosen === this.selected ? this.details.scrollTop : 0;
      this.selected = chosen;
      this.lastDetails = performance.now();
      this.status.textContent = chosen !== null ? `Event #${chosen + 1}` :
        `${events.length} layout boxes${events.length === 256 ? " (limit)" : ""}`;
      this.details.innerHTML = debug && event ? describeEvent(event, debug, timeMs) :
        '<div class="placeholder">Hover a subtitle or focus an active event to inspect it. For overlapping boxes, hover selects the last painted event; use the event cards to inspect each layer.</div>';
      this.details.scrollTop = scroll;
    }
  }
}
