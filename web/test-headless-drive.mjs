// Drives the headless wasm client with keyboard input so we can verify
// that the player ship, thrust flame, HP bar, and scrolling camera all
// render. Holds Up + Right for a stretch, takes a screenshot, then
// optionally another after another stretch.
//
//   node web/test-headless-drive.mjs [URL] [OUT] [HOLD_MS]
// Defaults: URL=http://127.0.0.1:4010/, OUT=/tmp/icarust-drive.png, HOLD_MS=2500
import { chromium } from 'playwright';

const URL = process.argv[2] ?? 'http://127.0.0.1:4010/';
const OUT = process.argv[3] ?? '/tmp/icarust-drive.png';
const HOLD_MS = Number(process.argv[4] ?? '2500');
const SETTLE_MS = Number(process.env.SETTLE_MS ?? '3000');

const browser = await chromium.launch({
  headless: true,
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
// Just enough time for wasm to boot + the connect/Welcome roundtrip.
await page.waitForTimeout(SETTLE_MS);

// Focus the canvas, then start thrusting immediately so gravity doesn't
// drag the player into the ground while we wait.
const canvas = await page.$('canvas');
if (canvas) await canvas.focus();
await page.keyboard.down('ArrowUp');
// Then chase down/right for a bit so the camera scrolls visibly.
await page.keyboard.down('ArrowRight');
await page.waitForTimeout(HOLD_MS);
await page.keyboard.up('ArrowRight');
// One more straight-up burst to show the flame plume cleanly.
await page.waitForTimeout(450);

await page.screenshot({ path: OUT });
await page.keyboard.up('ArrowUp');
await browser.close();
for (const l of lines.slice(-30)) console.log(' ', l);
console.log('Screenshot:', OUT);
