// Verifies the new time-based wave director by keeping the pilot alive
// long enough for two level transitions. Watches the console for LevelUp
// events (the client doesn't surface them as text, so we cross-check by
// reading the on-screen HUD via OCR-ish hash sampling — no, simpler: we
// just take screenshots and rely on the eye / HUD text).
//
//   node web/test-level-progression.mjs [URL] [OUT_DIR]
//
// Strategy: drive the pilot upward at all times so they hover near the
// ceiling and don't fly into the ground. Take a screenshot at t=2s (HUD
// should read Level 1, no tanks visible), at t=32s (should show Level 2,
// still no tanks), and t=64s (Level 3+, tanks should start appearing).

import { chromium } from 'playwright';
import { mkdir } from 'node:fs/promises';

const URL = process.argv[2] ?? 'http://127.0.0.1:4010/';
const OUT_DIR = process.argv[3] ?? '/tmp/icarust-levels';
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
await page.waitForTimeout(4000);

const canvas = await page.$('canvas');
if (canvas) await canvas.click();
await page.waitForTimeout(200);

// Launch into the game.
await page.keyboard.press('Space');
await page.waitForTimeout(500);

// Snap right after launch — level 1, no tanks.
await page.screenshot({ path: `${OUT_DIR}/t-00s-start.png` });

// Respawn cycle that's safe in any state. In Menu, Space launches; in
// GameOver, KeyA returns to menu; in Playing, both are harmless (Space
// fires, KeyA is unbound). Chained: KeyA → Space → Space brings us from
// any state back to Playing.
const respawn = async () => {
  await page.keyboard.press('KeyA');
  await page.waitForTimeout(80);
  await page.keyboard.press('Space');
  await page.waitForTimeout(80);
  await page.keyboard.press('Space');
  await page.waitForTimeout(80);
};

// Pulse-thrust loop: press Up briefly to fight gravity, then release so
// the next key press can register cleanly. Cheaper than holding Up across
// state transitions (key state is per-AppState in the client).
const tickGameplay = async (totalMs) => {
  const deadline = Date.now() + totalMs;
  while (Date.now() < deadline) {
    await respawn();
    await page.keyboard.down('ArrowUp');
    await page.waitForTimeout(400);
    await page.keyboard.up('ArrowUp');
    await page.waitForTimeout(100);
  }
};

await tickGameplay(2000);
await page.screenshot({ path: `${OUT_DIR}/t-02s-level1.png` });

await tickGameplay(30000);
await page.screenshot({ path: `${OUT_DIR}/t-32s-level2.png` });

await tickGameplay(32000);
await page.screenshot({ path: `${OUT_DIR}/t-64s-level3.png` });
await browser.close();

// Surface anything interesting from the log.
const interesting = lines.filter((l) => /pageerror|panic|ERROR|level/i.test(l));
for (const l of interesting.slice(-30)) console.log(' ', l);
console.log('Screenshots in', OUT_DIR);
