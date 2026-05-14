// Headless wasm smoke test for the Icarust web client.
//
// ggez 0.10+ is WebGPU-only on the web. Headless Chromium does NOT expose
// a WebGPU adapter without the flags below — the Playwright MCP server and
// a vanilla `chromium.launch()` will both bail with "No available adapters."
// and ggez panics with `GraphicsInitializationError`. We launch Chromium
// ourselves with the flags Linux needs to wire WebGPU through Vulkan/ANGLE.
//
// Usage: node web/test-headless.mjs [URL] [SCREENSHOT_PATH]
//   URL              defaults to http://127.0.0.1:4010/
//   SCREENSHOT_PATH  defaults to /tmp/icarust.png
//   WAIT_MS env var  defaults to 6000 — bump for slow first-frame paths.
//
// If `playwright` isn't already in the closest node_modules, install once:
//   cd web && npm i -D playwright && npx playwright install chromium

import { chromium } from 'playwright';

const URL = process.argv[2] ?? 'http://127.0.0.1:4010/';
const OUT = process.argv[3] ?? '/tmp/icarust.png';
const WAIT_MS = Number(process.env.WAIT_MS ?? '6000');

const browser = await chromium.launch({
  headless: true,
  // Linux needs all of these to expose a WebGPU adapter in headless Chromium.
  // Other platforms only need --enable-unsafe-webgpu, but the rest are harmless.
  args: [
    '--enable-unsafe-webgpu',
    '--enable-features=Vulkan',
    '--use-angle=vulkan',
    '--disable-vulkan-fallback-to-gl-for-testing',
    '--ignore-gpu-blocklist',
  ],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
const lines = [];
page.on('console', (m) => lines.push(`[${m.type()}] ${m.text()}`));
page.on('pageerror', (e) => lines.push(`[pageerror] ${e.message}`));

await page.goto(URL, { waitUntil: 'load' });
await page.waitForTimeout(WAIT_MS);

const state = await page.evaluate(async () => ({
  webgpu: !!navigator.gpu,
  hasAdapter: !!(navigator.gpu && (await navigator.gpu.requestAdapter().catch(() => null))),
  canvases: [...document.querySelectorAll('canvas')].map((c) => ({ w: c.width, h: c.height, id: c.id })),
}));
await page.screenshot({ path: OUT });
await browser.close();
console.log('STATE:', JSON.stringify(state, null, 2));
for (const l of lines) console.log(' ', l);
console.log('Screenshot:', OUT);
