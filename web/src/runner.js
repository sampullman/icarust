// Boots the Icarust wasm client. Fetches the resources zip (so ggez's
// `Filesystem::new_web` can mount it via `window.__GGEZ_RESOURCES_ZIP__`),
// then dynamically imports the wasm-bindgen JS shim and awaits its
// default-exported `init()` call.

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
  await mod.default();
  setStatus('');
} catch (err) {
  console.error(err);
  setStatus(`error: ${err}`);
}
