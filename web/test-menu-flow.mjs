// Drive the menu → playing → game-over → menu flow.
//
// Visits each transition and shoots a screenshot so a human (or me) can
// eyeball that the right thing rendered. Mirrors test-headless.mjs in
// boot setup but drives keyboard input at the page level.

import { chromium } from 'playwright';

const URL = process.argv[2] ?? 'http://127.0.0.1:4010/';
const OUT_DIR = process.argv[3] ?? '/tmp/icarust-menu-flow';
const WAIT_MS = Number(process.env.WAIT_MS ?? '5000');

import { mkdir } from 'node:fs/promises';
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
await page.waitForTimeout(WAIT_MS);

// Focus the canvas so keystrokes route to ggez instead of the document body.
const canvas = await page.$('canvas');
if (canvas) {
  await canvas.click();
} else {
  console.warn('!! no canvas found, keyboard events may not register');
}
await page.waitForTimeout(200);

// 1. Initial menu — title screen with ships flying.
await page.screenshot({ path: `${OUT_DIR}/01-menu.png` });

// 2. Press Space → should launch into Playing.
await page.keyboard.press('Space');
await page.waitForTimeout(800);
await page.screenshot({ path: `${OUT_DIR}/02-playing-just-launched.png` });

// 3. Wait long enough for the player to die (no input → enemies kill them).
await page.waitForTimeout(8000);
await page.screenshot({ path: `${OUT_DIR}/03-after-wait.png` });

// 4. Press any key → return to menu.
await page.keyboard.press('KeyA');
await page.waitForTimeout(600);
await page.screenshot({ path: `${OUT_DIR}/04-after-keypress.png` });

// 5. Press Space → respawn into Playing.
await page.keyboard.press('Space');
await page.waitForTimeout(800);
await page.screenshot({ path: `${OUT_DIR}/05-respawned.png` });

await browser.close();
console.log('Screenshots in', OUT_DIR);
for (const l of lines.slice(-30)) console.log(' ', l);
