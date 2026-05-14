// Memory-growth probe. Boots the wasm client, plays for SETTLE_MS so the
// world has rocks/enemies/shots flying, then dispatches a synthetic
// `visibilitychange` so the app's hidden-tab gating engages (chrome doesn't
// actually background a headless tab, but `document.visibilityState` can be
// patched and the event dispatched). We sample wasm linear memory and the
// CDP-exposed `JSHeapUsedSize` every second and report growth rate.
//
//   node web/test-headless-mem.mjs [URL] [SETTLE_MS] [SAMPLE_MS] [DURATION_MS]
// Defaults: URL=http://127.0.0.1:4010/, SETTLE=4000, SAMPLE=1000, DURATION=20000

import { chromium } from 'playwright';

const URL = process.argv[2] ?? 'http://127.0.0.1:4010/';
const SETTLE_MS = Number(process.argv[3] ?? '4000');
const SAMPLE_MS = Number(process.argv[4] ?? '1000');
const DURATION_MS = Number(process.argv[5] ?? '20000');

const browser = await chromium.launch({
  headless: true,
  args: [
    '--enable-unsafe-webgpu',
    '--enable-features=Vulkan',
    '--use-angle=vulkan',
    '--disable-vulkan-fallback-to-gl-for-testing',
    '--ignore-gpu-blocklist',
    // Disable rAF throttling so a "hidden" tab still gets full CPU and we
    // measure the *work* the app does, not what Chrome's background policy
    // would let through. Without this the leak would look ~60× smaller.
    '--disable-background-timer-throttling',
    '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows',
    // Skip the user-gesture requirement so AudioContext starts immediately —
    // we need it actually running so play_detached takes the leaky path.
    '--autoplay-policy=no-user-gesture-required',
  ],
});
const ctx = await browser.newContext({ viewport: { width: 1280, height: 720 } });
const page = await ctx.newPage();
const lines = [];
page.on('console', (m) => lines.push(`[${m.type()}] ${m.text()}`));
page.on('pageerror', (e) => lines.push(`[pageerror] ${e.message}`));

await page.goto(URL, { waitUntil: 'load' });
await page.waitForTimeout(SETTLE_MS);

// Unlock audio so play_detached actually leaks rodio sinks (the very leak
// we're hunting). Without a gesture, AudioContext stays suspended and
// ggez's `play_detached` is a no-op.
const canvas = await page.$('canvas');
if (canvas) await canvas.focus();
await page.keyboard.down('Space');
await page.waitForTimeout(150);
await page.keyboard.up('Space');
// Hold Up + Space so the player keeps thrusting (smoke trail, frequent
// shots → frequent `play_sound` calls). This is the high-allocation path.
await page.keyboard.down('ArrowUp');
await page.keyboard.down('Space');
await page.waitForTimeout(500);

// Simulate the tab going to the background. `document.hidden` is read-only
// from page script, so override its getter and dispatch `visibilitychange`
// — that's what window/event listeners observe.
async function setHidden(hidden) {
  await page.evaluate((h) => {
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => h });
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      get: () => (h ? 'hidden' : 'visible'),
    });
    document.dispatchEvent(new Event('visibilitychange'));
  }, hidden);
}

const cdp = await ctx.newCDPSession(page);
await cdp.send('Performance.enable');
await cdp.send('HeapProfiler.enable');

async function getCdpMetrics() {
  const { metrics } = await cdp.send('Performance.getMetrics');
  const map = Object.fromEntries(metrics.map((m) => [m.name, m.value]));
  return {
    jsHeapUsed: map.JSHeapUsedSize ?? 0,
    jsHeapTotal: map.JSHeapTotalSize ?? 0,
    docs: map.Documents ?? 0,
    nodes: map.Nodes ?? 0,
    listeners: map.JSEventListeners ?? 0,
  };
}
async function sample() {
  // Wasm linear memory is reachable from the WebAssembly.Memory instance
  // wasm-bindgen left attached to the loaded module. We dig it out via the
  // global the runner set on init. If anything's missing, fall back to 0.
  // runner.js stashes the wasm memory on `window.__icarustWasmMemory` after
  // init for exactly this purpose (the export is otherwise private to the
  // wasm-bindgen JS shim).
  const wasm = await page.evaluate(() =>
    (globalThis.__icarustWasmMemory && globalThis.__icarustWasmMemory.buffer.byteLength) || 0,
  );
  // CDP Performance.getMetrics is far more precise than performance.memory
  // (which Chrome rounds heavily for fingerprinting), and surfaces the
  // listener/node counts that tell us if anything is accumulating.
  const m = await getCdpMetrics();
  return { wasm, ...m };
}

const samples = [];
async function snapshot(label) {
  const t = Date.now();
  const s = await sample();
  samples.push({ t, label, ...s });
  console.log(
    `${label}\tt=${(t - t0).toString().padStart(6)}ms\twasm=${(s.wasm / 1024 / 1024).toFixed(2)}MB` +
      `\tjsUsed=${(s.jsHeapUsed / 1024).toFixed(0)}KB` +
      `\tlisteners=${s.listeners}\tnodes=${s.nodes}`,
  );
}

const HIDE = process.env.HIDE !== '0';
const t0 = Date.now();
await snapshot('start');
if (HIDE) {
  await setHidden(true);
  await snapshot('hidden');
} else {
  await snapshot('foreground');
}

const end = t0 + DURATION_MS;
while (Date.now() < end) {
  await page.waitForTimeout(SAMPLE_MS);
  await snapshot('tick');
}

await setHidden(false);
await page.waitForTimeout(500);
await snapshot('shown');

await browser.close();

// Growth rate: linear fit between the first `hidden` and the last `tick`.
const first = samples.find((s) => s.label === 'hidden' || s.label === 'foreground');
const last = [...samples].reverse().find((s) => s.label === 'tick');
if (first && last) {
  const dt = (last.t - first.t) / 1000;
  const dWasm = (last.wasm - first.wasm) / 1024;
  const dJs = (last.jsHeapUsed - first.jsHeapUsed) / 1024;
  console.log('---');
  console.log(`hidden duration:   ${dt.toFixed(1)} s`);
  console.log(`wasm grew:         ${dWasm.toFixed(1)} KB  (${(dWasm / dt).toFixed(1)} KB/s)`);
  console.log(`js heap grew:      ${dJs.toFixed(1)} KB  (${(dJs / dt).toFixed(1)} KB/s)`);
}
for (const l of lines.slice(-10)) console.log(' ', l);
