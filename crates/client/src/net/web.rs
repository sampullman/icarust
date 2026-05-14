//! Browser `Net` impl. Wraps a `web_sys::WebSocket`.
//!
//! Inbound messages are pushed to `VecDeque<ServerMsg>` by the `onmessage` callback;
//! `try_recv` pops from the front. Outbound messages call `send_with_u8_array`
//!  when the socket is open, or queue in a pending buffer (to wait for `onopen`)
//!
//! Inbound queue is capped at `MAX_RX_QUEUE`. Browsers throttle `requestAnimationFrame`
//! on hidden tabs but keep firing `onmessage` at the server's full rate, so a background
//! tab can grow the queue by ~80 msg/sec forever. We drop the oldest message on overflow,
//! snapshots are full-state, so losing old ones costs nothing.
//!
//! JS runs things on the main thread and `wasm32-unknown-unknown` has no threads.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use anyhow::{anyhow, Result};
use js_sys::Uint8Array;
use protocol::{ClientMsg, ServerMsg};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{BinaryType, ErrorEvent, MessageEvent, WebSocket};

use super::Net;

/// ~1s of server traffic; old messages drop on overflow.
const MAX_RX_QUEUE: usize = 128;

/// Check `document.hidden` without panicking if `window`/`document` are
/// somehow gone. Cheap: a couple of JS calls per WebSocket frame.
fn document_hidden() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .map(|d| d.hidden())
        .unwrap_or(false)
}

/// Inner state, shared between the `WebNet` handle and the JS callbacks
/// installed on the socket. Kept in a `Rc<RefCell<_>>` because callbacks
/// outlive any single function and JS hands control back to us on its own
/// schedule.
struct Inner {
    ws: WebSocket,
    rx: VecDeque<ServerMsg>,
    /// Anything we tried to send before `onopen` fires gets buffered here
    /// and flushed when the socket is ready.
    pending_tx: Vec<Vec<u8>>,
    connected: bool,
    /// Set once `onopen` has run so subsequent sends bypass the queue.
    open: bool,
}

pub struct WebNet {
    inner: Rc<RefCell<Inner>>,
    /// Keep the closures alive for the lifetime of the connection. Dropping
    /// them would deregister the callbacks immediately.
    _on_open: Closure<dyn FnMut(JsValue)>,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
    _on_close: Closure<dyn FnMut(JsValue)>,
}

impl WebNet {
    pub fn connect(url: &str, name: String) -> Result<Self> {
        let ws = WebSocket::new(url).map_err(js_err("WebSocket::new"))?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let inner = Rc::new(RefCell::new(Inner {
            ws: ws.clone(),
            rx: VecDeque::new(),
            pending_tx: Vec::new(),
            connected: true,
            open: false,
        }));

        // onopen: flush any buffered Hello/Input frames.
        let inner_open = inner.clone();
        let on_open = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
            let mut s = inner_open.borrow_mut();
            s.open = true;
            // Drain `pending_tx`. If a send fails we drop the rest — the
            // socket is probably already dead.
            let pending = std::mem::take(&mut s.pending_tx);
            for buf in pending {
                if let Err(e) = s.ws.send_with_u8_array(&buf) {
                    web_sys::console::warn_1(
                        &format!("ws send_with_u8_array failed: {e:?}").into(),
                    );
                    s.connected = false;
                    break;
                }
            }
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));

        // onmessage: decode the binary payload into a ServerMsg and push it
        // into the queue. Text frames and other types are ignored. While the
        // tab is hidden we still drain the network frame (so the browser
        // doesn't buffer it indefinitely on its side), but skip the decode
        // + allocations entirely — `update` won't process queued messages
        // anyway, and on un-hide we just resync to the next snapshot.
        let inner_msg = inner.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
            if document_hidden() {
                return;
            }
            let data = evt.data();
            // We requested ArrayBuffer above; anything else means the server
            // is doing something unexpected.
            let Ok(buffer) = data.dyn_into::<js_sys::ArrayBuffer>() else {
                web_sys::console::warn_1(&"ws got non-ArrayBuffer message".into());
                return;
            };
            let arr = Uint8Array::new(&buffer);
            let bytes = arr.to_vec();
            match protocol::decode::<ServerMsg>(&bytes) {
                Ok(msg) => {
                    let mut s = inner_msg.borrow_mut();
                    while s.rx.len() >= MAX_RX_QUEUE {
                        s.rx.pop_front();
                    }
                    s.rx.push_back(msg);
                }
                Err(e) => {
                    web_sys::console::warn_1(&format!("malformed server message: {e}").into())
                }
            }
        });
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let inner_err = inner.clone();
        let on_error = Closure::<dyn FnMut(ErrorEvent)>::new(move |evt: ErrorEvent| {
            web_sys::console::warn_1(&format!("ws error: {}", evt.message()).into());
            inner_err.borrow_mut().connected = false;
        });
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        let inner_close = inner.clone();
        let on_close = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
            inner_close.borrow_mut().connected = false;
        });
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let net = WebNet {
            inner: inner.clone(),
            _on_open: on_open,
            _on_message: on_message,
            _on_error: on_error,
            _on_close: on_close,
        };

        // Send Hello first so the server can place us in the world the moment
        // the WS handshake finishes. The socket isn't open yet — `send` will
        // buffer until onopen flushes it.
        net.send(&ClientMsg::Hello { name });

        Ok(net)
    }
}

impl Net for WebNet {
    fn try_recv(&mut self) -> Option<ServerMsg> {
        self.inner.borrow_mut().rx.pop_front()
    }

    fn send(&self, msg: &ClientMsg) {
        let mut s = self.inner.borrow_mut();
        if !s.connected {
            return;
        }
        let bytes = protocol::encode(msg);
        if s.open {
            if let Err(e) = s.ws.send_with_u8_array(&bytes) {
                web_sys::console::warn_1(&format!("ws send failed: {e:?}").into());
                s.connected = false;
            }
        } else {
            s.pending_tx.push(bytes);
        }
    }

    fn is_connected(&self) -> bool {
        self.inner.borrow().connected
    }
}

fn js_err(ctx: &'static str) -> impl FnOnce(JsValue) -> anyhow::Error {
    move |v| {
        anyhow!(
            "{ctx}: {:?}",
            v.as_string().unwrap_or_else(|| format!("{v:?}"))
        )
    }
}
