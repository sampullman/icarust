// Visual smoke for the new bullet behavior. Launches into the game, points
// the ship downward by holding Right past 180°, then fires Space repeatedly
// while thrusting up so the bullets head toward the ground. With the new
// rules those bullets should be absorbed by terrain, not bounce.

import { chromium } from 'playwright';

const URL = process.argv[2] ?? 'http://127.0.0.1:4010/';
const OUT = process.argv[3] ?? '/tmp/icarust-bullets.png';

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
await page.waitForTimeout(3500);
const canvas = await page.$('canvas');
if (canvas) await canvas.click();
await page.waitForTimeout(200);
await page.keyboard.press('Space'); // launch
await page.waitForTimeout(400);

// Rotate the ship halfway round so the nose points down. Turn rate is
// 3 rad/s, so ~1.05s of held Right covers about pi radians.
await page.keyboard.down('ArrowRight');
await page.waitForTimeout(1050);
await page.keyboard.up('ArrowRight');

// Thrust upward (we're pointing down now, so thrust pushes us up — wait,
// thrust is along facing, so pointing-down + thrust = downward thrust.
// Instead we just fire and let the bullet travel downward).
for (let i = 0; i < 5; i++) {
  await page.keyboard.press('Space');
  await page.waitForTimeout(120);
}
await page.waitForTimeout(700);
await page.screenshot({ path: OUT });
await browser.close();

for (const l of lines.slice(-10)) console.log(' ', l);
console.log('Screenshot:', OUT);
