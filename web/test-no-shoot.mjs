// Forces GC between samples so the numbers reflect *retained* memory under
// a steady fire/thrust load — not transient GC peaks. Useful for finding
// slow leaks the regular probe misses.
import { chromium } from 'playwright';

const browser = await chromium.launch({
  headless: true,
  args: ['--enable-unsafe-webgpu','--enable-features=Vulkan','--use-angle=vulkan',
    '--disable-vulkan-fallback-to-gl-for-testing','--ignore-gpu-blocklist',
    '--disable-background-timer-throttling','--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows',
    ...(process.env.NOAUTO ? [] : ['--autoplay-policy=no-user-gesture-required'])],
});
const ctx = await browser.newContext({ viewport: { width: 1280, height: 720 } });
const page = await ctx.newPage();
const cdp = await ctx.newCDPSession(page);
await cdp.send('Performance.enable');
await cdp.send('HeapProfiler.enable');
async function gcAndMetrics() {
  await cdp.send('HeapProfiler.collectGarbage');
  const { metrics: m } = await cdp.send('Performance.getMetrics');
  const x = Object.fromEntries(m.map((mm) => [mm.name, mm.value]));
  const wasm = await page.evaluate(() =>
    (globalThis.__icarustWasmMemory && globalThis.__icarustWasmMemory.buffer.byteLength) || 0
  );
  return { listeners: x.JSEventListeners, heap: x.JSHeapUsedSize, wasm };
}
await page.goto('http://127.0.0.1:4010/', { waitUntil: 'load' });
await page.waitForTimeout(5000);
const canvas = await page.$('canvas');
if (canvas) await canvas.focus();
await page.keyboard.down('ArrowUp');
await page.keyboard.down('Space');

const t0 = Date.now();
const baseline = await gcAndMetrics();
console.log('baseline:', baseline);
for (let i = 0; i < 12; i++) {
  await page.waitForTimeout(5000);
  const m = await gcAndMetrics();
  console.log(
    `t=${((Date.now() - t0)/1000).toFixed(0)}s` +
    ` heap=${(m.heap/1024).toFixed(0)}KB (Δ${((m.heap-baseline.heap)/1024).toFixed(0)})` +
    ` wasm=${(m.wasm/1024/1024).toFixed(2)}MB (Δ${((m.wasm-baseline.wasm)/1024).toFixed(0)}KB)` +
    ` listeners=${m.listeners} (Δ${m.listeners-baseline.listeners})`
  );
}
await browser.close();
