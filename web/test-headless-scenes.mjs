// Capture a few different scenes so we can visually QA the new HUD/ship/
// thrust/smoke/camera features. Each shot waits for a different game
// state to develop, then snaps.
//
//   node web/test-headless-scenes.mjs [URL] [OUT_DIR]
// Defaults: URL=http://127.0.0.1:4010/, OUT_DIR=/tmp
import { chromium } from 'playwright';
import { mkdir } from 'node:fs/promises';

const URL = process.argv[2] ?? 'http://127.0.0.1:4010/';
const OUT_DIR = process.argv[3] ?? '/tmp';
await mkdir(OUT_DIR, { recursive: true });

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
await page.waitForTimeout(600);
const canvas = await page.$('canvas');
if (canvas) await canvas.focus();

// Scene 1: thrust + scrolling. Up + Right.
await page.keyboard.down('ArrowUp');
await page.keyboard.down('ArrowRight');
await page.waitForTimeout(1500);
await page.screenshot({ path: `${OUT_DIR}/icarust-scene-thrust.png` });

// Scene 2: hold thrust longer + fire so we kill rocks and provoke shots.
await page.keyboard.down('Space');
await page.waitForTimeout(1800);
await page.screenshot({ path: `${OUT_DIR}/icarust-scene-fire.png` });
await page.keyboard.up('Space');

// Scene 3: stop fighting, dive toward terrain to show ground crash + smoke
// trail if any damage built up.
await page.keyboard.up('ArrowRight');
await page.keyboard.up('ArrowUp');
await page.waitForTimeout(2400);
await page.screenshot({ path: `${OUT_DIR}/icarust-scene-falling.png` });

await browser.close();
for (const l of lines.slice(-10)) console.log(' ', l);
console.log('Done.');
