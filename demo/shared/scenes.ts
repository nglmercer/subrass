/** Scene boundaries and representative seek times in the bundled sample. */
export const sampleScenes = [
  { start: 0, seek: 0, label: "Introduction" },
  { start: 10000, seek: 11000, label: "Karaoke hard swap" },
  { start: 25000, seek: 26000, label: "Karaoke sweep" },
  { start: 40000, seek: 41000, label: "Karaoke outline" },
  { start: 52000, seek: 53000, label: "Combined karaoke" },
  { start: 65000, seek: 67000, label: "Colors & outlines",
    note: "In the opening example, magenta fill and yellow outline are intentional. The rows use explicit script positions." },
  { start: 80000, seek: 81000, label: "Font formatting" },
  { start: 95000, seek: 96000, label: "Alignment" },
  { start: 110000, seek: 112000, label: "Movement" },
  { start: 130000, seek: 132000, label: "Transforms" },
  { start: 150000, seek: 152000, label: "Borders & blur" },
  { start: 165000, seek: 166000, label: "Clipping" },
  { start: 180000, seek: 182000, label: "Fades" },
  { start: 195000, seek: 197000, label: "Vector drawings" },
  { start: 215000, seek: 217000, label: "Simultaneous dialogue" },
  { start: 230000, seek: 231000, label: "Combined overrides" },
  { start: 245000, seek: 246000, label: "Style reset" },
  { start: 255000, seek: 256000, label: "Legacy SSA styles" },
  { start: 270000, seek: 271000, label: "Multiline text" },
  { start: 285000, seek: 287000, label: "Overlapping layers",
    note: "These labels overlap intentionally. Back/front describes paint order, not vertical placement. Hover picks the last painted layout box; use the event cards to inspect each layer." },
  { start: 295000, seek: 296000, label: "Outro" },
];

export function sampleSceneAt(timeMs: number) {
  return sampleScenes.findLast(scene => timeMs >= scene.start) ?? sampleScenes[0]!;
}
