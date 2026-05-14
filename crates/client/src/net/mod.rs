//! Net trait and target-specific implementations.
//!
//! The trait is the only place the client code touches the network.
//! Everything else uses `&mut dyn Net`. Native uses a tokio-tungstenite
//! thread; wasm uses `web_sys::WebSocket` driven by the JS event loop.

use protocol::{ClientMsg, ServerMsg};

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeNet;

#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::WebNet;

/// Wasm types from `web-sys` are not `Send`; conditionally drop the bound.
/// Native impls still need to be `Send` because the connection runs on a
/// dedicated thread.
#[cfg(not(target_arch = "wasm32"))]
pub trait Net: Send {
    fn try_recv(&mut self) -> Option<ServerMsg>;
    fn send(&self, msg: &ClientMsg);
    fn is_connected(&self) -> bool;
}

#[cfg(target_arch = "wasm32")]
pub trait Net {
    fn try_recv(&mut self) -> Option<ServerMsg>;
    fn send(&self, msg: &ClientMsg);
    fn is_connected(&self) -> bool;
}
