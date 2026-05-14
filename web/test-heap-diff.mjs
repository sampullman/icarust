// Takes a heap snapshot at baseline and another after `STEADY_MS` of play,
// reports which constructor families grew. Helps narrow down what's leaking
// when the regular metrics probe says "JS heap grew but listeners didn't".
import { chromium } from 'playwright';
import { writeFileSync } from 'node:fs';

const STEADY_MS = Number(process.env.STEADY_MS ?? 60000);
const browser = await chromium.launch({
  headless: true,
  args: ['--enable-unsafe-webgpu','--enable-features=Vulkan','--use-angle=vulkan',
    '--disable-vulkan-fallback-to-gl-for-testing','--ignore-gpu-blocklist',
    '--disable-background-timer-throttling','--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows','--autoplay-policy=no-user-gesture-required'],
});
const ctx = await browser.newContext({ viewport: { width: 1280, height: 720 } });
const page = await ctx.newPage();
const cdp = await ctx.newCDPSession(page);
await cdp.send('HeapProfiler.enable');

// Save a v8 heap snapshot to a string via streaming chunks.
async function takeSnap(path) {
  await cdp.send('HeapProfiler.collectGarbage');
  const chunks = [];
  const onChunk = (e) => chunks.push(e.chunk);
  cdp.on('HeapProfiler.addHeapSnapshotChunk', onChunk);
  await cdp.send('HeapProfiler.takeHeapSnapshot', { reportProgress: false, captureNumericValue: false });
  cdp.off('HeapProfiler.addHeapSnapshotChunk', onChunk);
  const text = chunks.join('');
  writeFileSync(path, text);
  return JSON.parse(text);
}

// Build a constructor-name → totalShallowSize histogram. The v8 snapshot
// format is a flat array of node fields; we walk it by `node_count`.
function histogram(snap) {
  const meta = snap.snapshot.meta;
  const nf = meta.node_fields;
  const nt = meta.node_types[nf.indexOf('type')];  // string list of types
  const idxType = nf.indexOf('type');
  const idxName = nf.indexOf('name');
  const idxSelf = nf.indexOf('self_size');
  const stride = nf.length;
  const strings = snap.strings;
  const nodes = snap.nodes;
  const out = new Map();
  for (let i = 0; i < nodes.length; i += stride) {
    const tIdx = nodes[i + idxType];
    const t = nt[tIdx];
    const name = strings[nodes[i + idxName]] || `(${t})`;
    const sz = nodes[i + idxSelf];
    const key = `${t}:${name}`;
    out.set(key, (out.get(key) || 0) + sz);
  }
  return out;
}

await page.goto('http://127.0.0.1:4010/', { waitUntil: 'load' });
await page.waitForTimeout(5000);
const canvas = await page.$('canvas');
if (canvas) await canvas.focus();
await page.keyboard.down('ArrowUp');
await page.keyboard.down('Space');
await page.waitForTimeout(2000);

console.log('Taking baseline heap snapshot…');
const before = histogram(await takeSnap('/tmp/heap-before.json'));
console.log(`Idling ${STEADY_MS / 1000}s under steady fire+thrust…`);
await page.waitForTimeout(STEADY_MS);
console.log('Taking final heap snapshot…');
const after = histogram(await takeSnap('/tmp/heap-after.json'));

const rows = [];
const keys = new Set([...before.keys(), ...after.keys()]);
for (const k of keys) {
  const a = before.get(k) || 0;
  const b = after.get(k) || 0;
  const delta = b - a;
  if (delta > 1024) rows.push({ k, delta, before: a, after: b });
}
rows.sort((x, y) => y.delta - x.delta);
console.log(`Top growers (Δ ≥ 1KB) after ${STEADY_MS / 1000}s:`);
for (const r of rows.slice(0, 25)) {
  console.log(`  +${(r.delta / 1024).toFixed(1).padStart(8)}KB  ${r.k}   (${(r.before/1024).toFixed(0)} → ${(r.after/1024).toFixed(0)})`);
}
await browser.close();
