import { expect, test } from "bun:test";
import { describeEvent, framePoint, hitTest } from "../demo/shared/inspector.ts";
import type { AssEvent, EventDebug } from "../demo/shared/types.ts";

const debug: EventDebug = { event_index: 0, layout_bounds: [10, 20, 70, 80],
  collision_eligible: false, collision_shift: 0, alignment: 5, origin: [40, 50],
  opacity: 1, run_count: 1, runs: [{ font_name: "DejaVu Sans", font_size: 48,
    fill: [255, 0, 0, 255], outline_color: [0, 0, 0, 255], scale: [100, 50],
    rotation: [0, 20, 30], shear: [0.2, 0], border: [2, 3], shadow: [1, 2],
    blur: 1.5, edge_blur: 2, drawing_mode: 0 }] };

test("hover maps independent CSS axes and chooses the last painted overlapping event", () => {
  const point = framePoint(120, 80, { left: 20, top: 30, width: 200, height: 100 }, 100, 100);
  expect(point).toEqual([50, 50]);
  const top = { ...debug, event_index: 1 };
  expect(hitTest([debug, top], ...point)?.event_index).toBe(1);
  expect(hitTest([debug], 5, 50)).toBeUndefined();
  expect(hitTest([{ ...debug, layout_bounds: [NaN, 0, 70, 80] }], 50, 50)).toBeUndefined();
});

test("inspector distinguishes source fields and effective run values and escapes uploaded text", () => {
  const event: AssEvent = { layer: 2, style: '<style>', name: '<img src=x onerror=alert(1)>',
    start: { hours: 0, minutes: 1, seconds: 5, centiseconds: 0 },
    end: { hours: 0, minutes: 1, seconds: 9, centiseconds: 0 },
    margin_l: 0, margin_r: 0, margin_v: 0, effect: "", text: '{\\fs48}<script>alert(1)</script>' };
  const html = describeEvent(event, debug, 67000);
  expect(html).toContain("DejaVu Sans / 48");
  expect(html).toContain("100 / 50");
  expect(html).toContain("1.5 / 2");
  expect(html).toContain("#ff0000");
  expect(html).toContain("Collision</dt><dd>Excluded");
  expect(html).toContain("&lt;script&gt;");
  expect(html).not.toContain("<script>");
  expect(html).not.toContain("<img");
});
