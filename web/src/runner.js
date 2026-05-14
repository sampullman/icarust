// Boots the Icarust wasm client. Fetches the resources zip (so ggez's
// `Filesystem::new_web` can mount it via `window.__GGEZ_RESOURCES_ZIP__`),
// then dynamically imports the wasm-bindgen JS shim and awaits its
// default-exported `init()` call.

// Must run before the wasm import: wasm-bindgen captures `AudioContext` at
// module load, so rodio's context (created during wasm init) only inherits
// our subclass if the patch is in place beforehand. The subclass mirrors
// `AudioContext.state` onto `window.__ggezAudioState` so ggez's Rust-side
// `AudioContext::state()` / `is_running()` returns accurate values.
installGgezAudioUnlock();

const status = document.getElementById('status');
function setStatus(msg) {
  if (status) status.textContent = msg;
}

async function preloadResourcesZip() {
  try {
    const resp = await fetch('/resources.zip');
    if (!resp.ok) {
      console.warn(`resources.zip not available (HTTP ${resp.status}); asset loads will fail`);
      return;
    }
    window.__GGEZ_RESOURCES_ZIP__ = new Uint8Array(await resp.arrayBuffer());
  } catch (err) {
    console.warn('failed to preload resources.zip:', err);
  }
}

// See note above `installGgezAudioUnlock()` for why this lives in JS.
function installGgezAudioUnlock() {
  if (window.__ggezAudioUnlockInstalled) return;
  window.__ggezAudioUnlockInstalled = true;
  const Original = window.AudioContext || window.webkitAudioContext;
  if (!Original) return;
  class GgezUnlockingAudioContext extends Original {
    constructor() {
      super(...arguments);
      const sync = () => { window.__ggezAudioState = this.state; };
      sync();
      this.addEventListener('statechange', sync);
      if (this.state !== 'suspended') return;
      const unlock = () => { this.resume().catch(() => {}); };
      const opts = { capture: true, passive: true, once: true };
      for (const ev of ['pointerdown', 'keydown', 'touchstart', 'mousedown']) {
        addEventListener(ev, unlock, opts);
      }
    }
  }
  window.AudioContext = GgezUnlockingAudioContext;
  if (window.webkitAudioContext) window.webkitAudioContext = GgezUnlockingAudioContext;
}

try {
  setStatus('loading wasm…');
  await preloadResourcesZip();
  // Vite refuses to transform JS files under `public/` even with
  // `@vite-ignore`, so dodge its analyzer entirely: build a Blob URL
  // around the JS source we fetched, then dynamic-import that. The
  // Blob's import URL also lets the relative `client_bg.wasm` import
  // inside `client.js` resolve against the original location.
  const clientJsUrl = `/client/client.js`;
  const jsSrc = await (await fetch(clientJsUrl)).text();
  // Rewrite the relative import for client_bg.wasm to point at our
  // absolute path. wasm-bindgen emits `new URL('client_bg.wasm', import.meta.url)`.
  const rewritten = jsSrc.replaceAll(
    "new URL('client_bg.wasm', import.meta.url)",
    `'${new URL('/client/client_bg.wasm', location.href).href}'`,
  );
  const blobUrl = URL.createObjectURL(
    new Blob([rewritten], { type: 'application/javascript' }),
  );
  const mod = await import(/* @vite-ignore */ blobUrl);
  // wasm-bindgen --target web exposes init() as the default export. Awaiting it
  // instantiates the wasm and (because we use `#[wasm_bindgen(start)]`) runs
  // `wasm_start` which builds the ggez context and hands control to the
  // browser-driven event loop.
  const inst = await mod.default();
  // Expose the WebAssembly.Memory backing the wasm linear heap so test
  // harnesses can measure growth (it's otherwise captured inside the
  // module-private `wasm` variable inside client.js). Only used by
  // web/test-headless-mem.mjs — harmless in production.
  if (inst && inst.memory) window.__icarustWasmMemory = inst.memory;
  setStatus('');
} catch (err) {
  console.error(err);
  setStatus(`error: ${err}`);
}
