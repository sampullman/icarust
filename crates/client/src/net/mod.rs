//! Net trait and implementations.
//!
//! The trait is the only place the client code touches the network.
//! Everything else uses `&mut dyn Net`. A native impl is in `native.rs`;
//! a wasm impl is added in Phase 4.

use protocol::{ClientMsg, ServerMsg};

pub mod native;

pub use native::NativeNet;

pub trait Net: Send {
    /// Pull the next message from the network if one is ready. Non-blocking.
    fn try_recv(&mut self) -> Option<ServerMsg>;

    /// Queue a message to be sent. Best-effort; silently drops on disconnect.
    fn send(&self, msg: &ClientMsg);

    /// True until the underlying connection is gone.
    fn is_connected(&self) -> bool;
}
