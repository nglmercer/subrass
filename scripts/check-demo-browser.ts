// Real-browser integration checks. Start the demo and ChromeDriver first.
// bun run test:demo:browser (see DEMO-CHECK.md for environment options).
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";

const driver = process.env.WEBDRIVER_URL ?? "http://localhost:9515";
const web = process.env.DEMO_URL ?? "http://localhost:8001";
const artifacts = resolve(process.env.DEMO_ARTIFACT_DIR ?? "target/demo-browser");
await mkdir(artifacts, { recursive: true });
let session = "";
async function command(path: string, body?: unknown, method = body === undefined ? "GET" : "POST"): Promise<any> {
  const response = await fetch(`${driver}${session ? `/session/${session}` : ""}${path}`, {
    method, headers: { "Content-Type": "application/json" },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  });
  const result = await response.json() as { value: any };
  if (!response.ok || result.value?.error) throw new Error(JSON.stringify(result.value));
  return result.value;
}
const js = (script: string, ...args: unknown[]) => command("/execute/sync", { script, args });
async function waitFor(script: string): Promise<void> {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (await js(script)) return;
    await Bun.sleep(50);
  }
  throw new Error(`Browser condition timed out: ${script}`);
}
async function element(selector: string): Promise<string> {
  const value = await command("/element", { using: "css selector", value: selector });
  return value["element-6066-11e4-a52e-4f735466cecf"];
}
const click = async (selector: string) => command(`/element/${await element(selector)}/click`, {});
async function screenshot(name: string): Promise<void> {
  const data = await command("/screenshot");
  await Bun.write(`${artifacts}/${name}.png`, Buffer.from(data, "base64"));
}
const pixelHash = `const c=document.getElementById('subtitleCanvas');
  const data=c.getContext('2d').getImageData(0,0,c.width,c.height).data;
  let hash=2166136261; for(const b of data) hash=Math.imul(hash^b,16777619); return hash>>>0;`;

try {
  const options: Record<string, unknown> = { args: ["--headless=new", "--disable-dev-shm-usage"] };
  if (process.env.CHROME_BINARY) options.binary = process.env.CHROME_BINARY;
  if (process.env.CHROME_NO_SANDBOX === "1") (options.args as string[]).push("--no-sandbox");
  const created = await command("/session", { capabilities: { alwaysMatch: {
    browserName: "chrome", "goog:chromeOptions": options,
  } } });
  session = created.sessionId;
  await command("/timeouts", { script: 15000, pageLoad: 30000, implicit: 0 });
  for (const mode of ["basic", "worker"]) {
    await command("/window/rect", { width: 1440, height: 1100 });
    await command("/url", { url: `${web}/${mode}/` });
    await waitFor("return document.getElementById('summaryEvents').textContent === '110'");
    assert.equal(await js("return document.getElementById('error').style.display"), "none");
    assert.equal(await js("return document.documentElement.scrollWidth <= innerWidth"), true);
    assert.equal(await js("return document.getElementById('sceneSelect').options.length"), 21);
    // Reproduce the screenshots' slider seeks: the current scene must follow.
    await js("const s=document.getElementById('seekBar');s.value='213';s.dispatchEvent(new Event('input'));return true");
    assert.equal(await js("return document.getElementById('sceneSelect').value"), "67000");
    await js("const s=document.getElementById('sceneSelect');s.value='67000';s.dispatchEvent(new Event('change'));return true");
    await waitFor("return document.querySelectorAll('.event-item').length === 2");
    // Worker painting is asynchronous: inspection reply proves this frame was painted.
    await click("#inspectCheck");
    await waitFor("return document.querySelectorAll('.debug-box').length === 2");
    const before = await js(pixelHash);
    const center = await js(`const r=document.querySelector('.debug-box').getBoundingClientRect();
      return [Math.round(r.left+r.width/2),Math.round(r.top+r.height/2)];`);
    await command("/actions", { actions: [{ type: "pointer", id: "mouse", parameters: { pointerType: "mouse" },
      actions: [{ type: "pointerMove", duration: 0, origin: "viewport", x: center[0], y: center[1] }] }] });
    await waitFor("return !document.getElementById('hoverTooltip').hidden");
    assert.equal(await js("return document.getElementById('inspectorDetails').textContent.includes('Run 3')"), true);
    assert.equal(await js(pixelHash), before, "overlay must not modify the subtitle canvas");
    await screenshot(`${mode}-desktop-inspection`);
    // Leave the hit region before leaving the stage, like a real pointer path.
    await command("/actions", { actions: [{ type: "pointer", id: "mouse", parameters: { pointerType: "mouse" },
      actions: [{ type: "pointerMove", duration: 0, origin: "viewport", x: 100, y: 700 }] }] });
    assert.equal(await js("return document.getElementById('hoverTooltip').hidden"), true);
    assert.equal(await js("return document.getElementById('inspectorDetails').textContent.includes('Run 3')"), true);
    await command("/actions", { actions: [{ type: "pointer", id: "mouse", parameters: { pointerType: "mouse" },
      actions: [{ type: "pointerMove", duration: 0, origin: "viewport", x: 1150, y: 650 }] }] });
    assert.equal(await js("return document.getElementById('hoverTooltip').hidden"), true);
    assert.equal(await js("return document.getElementById('inspectorDetails').textContent.includes('Run 3')"), true,
      "moving into the inspector retains details for scrolling");
    await click("#inspectCheck");
    assert.equal(await js("return document.querySelectorAll('.debug-box').length"), 0);
    assert.equal(await js("return document.getElementById('inspectStatus').textContent"), "Off");
    assert.equal(await js("return document.getElementById('hoverTooltip').hidden"), true);
    assert.equal(await js(pixelHash), before);
    await screenshot(`${mode}-desktop`);
    // The layer example keeps its authored overlap and each card can be inspected.
    await js("const s=document.getElementById('seekBar');s.value='932';s.dispatchEvent(new Event('input'));return true");
    assert.equal(await js("return document.getElementById('sceneSelect').value"), "287000");
    assert.equal(await js("return document.getElementById('sceneNote').textContent.includes('overlap intentionally')"), true);
    await waitFor("return document.querySelectorAll('.event-item').length === 3");
    await click("#inspectCheck");
    await waitFor("return document.querySelectorAll('.debug-box').length === 3");
    await js("document.querySelector('.event-item').focus();return true");
    assert.equal(await js("return document.getElementById('inspectorDetails').textContent.includes('Layer 0 (back)')"), true);
    await screenshot(`${mode}-layers`);
    const front = await js(`const r=document.querySelectorAll('.debug-box')[2].getBoundingClientRect();
      return [Math.round(r.left+r.width/2),Math.round(r.top+r.height/2)];`);
    await command("/actions", { actions: [{ type: "pointer", id: "mouse", parameters: { pointerType: "mouse" },
      actions: [{ type: "pointerMove", duration: 0, origin: "viewport", x: front[0], y: front[1] }] }] });
    await waitFor("return document.getElementById('inspectStatus').textContent === 'Event #106'");
    assert.equal(await js("return document.getElementById('inspectorDetails').textContent.includes('Layer 2 (front)')"), true);
    await screenshot(`${mode}-layers-front`);
    // Narrow layout and source-byte upload paths use the same inspector.
    await command("/window/rect", { width: 390, height: 1100 });
    await js("window.scrollTo(0,0); return true");
    assert.equal(await js("return document.documentElement.scrollWidth <= innerWidth"), true, "mobile horizontal overflow");
    await screenshot(`${mode}-mobile`);
    const source = '[Script Info]\nPlayResX: 640\nPlayResY: 480\n[Events]\nDialogue: 0,0:00:00.00,0:06:00.00,Default,,0,0,0,,{\\pos(320,240)}UTF16 café <img src=x>\n';
    const file = `${artifacts}/upload.ass`;
    await Bun.write(file, Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(source, "utf16le")]));
    await command(`/element/${await element('#assInput')}/value`, { text: file });
    await waitFor("return document.getElementById('assName').textContent === 'upload.ass'");
    assert.equal(await js("return document.getElementById('sceneSelect').disabled"), true);
    await waitFor("return document.querySelectorAll('.debug-box').length === 1");
    await js("document.querySelector('.event-item').focus();return true");
    assert.equal(await js("return document.getElementById('inspectorDetails').textContent.includes('UTF16 café')"), true);
    assert.equal(await js("return document.querySelectorAll('#inspectorDetails img').length"), 0);
    await Bun.write(`${artifacts}/invalid.ass`, "[Script Info]\nPlayResX: nope");
    await command(`/element/${await element('#assInput')}/value`, { text: `${artifacts}/invalid.ass` });
    await waitFor("return document.getElementById('error').style.display === 'block'");
    assert.equal(await js("return document.getElementById('assName').textContent"), "upload.ass");
    // The overlay cap must not hide a selected card beyond its first 64 boxes.
    const dense = '[Script Info]\nPlayResX: 640\nPlayResY: 480\n[Events]\n' +
      Array.from({ length: 70 }, (_, i) => `Dialogue: ${i},0:00:00.00,0:06:00.00,Default,,0,0,0,,{\\pos(320,240)}Event ${i + 1}\n`).join('');
    await Bun.write(`${artifacts}/dense.ass`, dense);
    await command(`/element/${await element('#assInput')}/value`, { text: `${artifacts}/dense.ass` });
    await waitFor("return document.querySelectorAll('.debug-box').length === 64 && document.querySelectorAll('.event-item').length === 70");
    await js("document.querySelector('.event-item:last-child').focus();return true");
    assert.equal(await js("return document.querySelector('.debug-box.selected span').textContent"), "#70");
    assert.equal(await js("return document.querySelectorAll('.debug-box').length"), 64);
    assert.equal(await js("return document.getElementById('inspectStatus').textContent"), "Event #70");
    console.log(`PASS ${mode}: startup, scenes, hover, run values, pixel preservation, keyboard focus, mobile layout, UTF-16, failed reload`);
  }
  console.log(`Screenshots: ${artifacts}`);
} finally {
  if (session) await command("", undefined, "DELETE");
}
