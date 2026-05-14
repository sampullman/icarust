//! Native binary shim. All real logic lives in `lib.rs` so it can be reused
//! from the wasm `cdylib` build via `#[wasm_bindgen(start)]`.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> ggez::GameResult {
    client::native_main()
}

// On wasm we still need a `main` symbol for cargo to build the [[bin]] target,
// but the wasm entry point is `client::wasm_start` (registered via
// `wasm_bindgen(start)`). This main is never called.
#[cfg(target_arch = "wasm32")]
fn main() {}
