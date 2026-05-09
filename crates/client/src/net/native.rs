//! Native `Net` impl: a background thread runs a tokio runtime that owns
//! the WebSocket. The ggez main thread talks to it via two channels.
//!
//! - `to_net`: tokio mpsc, sync `send` from main, async `recv` in tokio.
//! - `from_net`: std mpsc, sync `try_recv` from main, sync `send` from tokio.
//!
//! The std mpsc on the inbound side keeps `try_recv` cheap and avoids
//! awaiting anything from inside the ggez `update` step.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMsg, ServerMsg};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use super::Net;

pub struct NativeNet {
    to_net: mpsc::UnboundedSender<ClientMsg>,
    from_net: std::sync::mpsc::Receiver<ServerMsg>,
    connected: Arc<AtomicBool>,
    _thread: thread::JoinHandle<()>,
}

impl NativeNet {
    /// Connect and send the initial `Hello`. Returns once the runtime
    /// thread is spawned — the WS connection itself is established
    /// asynchronously and the first message you receive will be `Welcome`.
    pub fn connect(url: String, name: String) -> Result<Self> {
        let (to_net_tx, to_net_rx) = mpsc::unbounded_channel::<ClientMsg>();
        let (from_net_tx, from_net_rx) = std::sync::mpsc::channel::<ServerMsg>();
        let connected = Arc::new(AtomicBool::new(true));
        let conn_for_thread = connected.clone();

        // Send Hello first so the server can place us in the world the moment
        // it sees the socket.
        to_net_tx
            .send(ClientMsg::Hello { name })
            .map_err(|_| anyhow::anyhow!("couldn't queue Hello"))?;

        let handle = thread::Builder::new()
            .name("icarust-net".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        warn!("failed to start tokio runtime: {e}");
                        conn_for_thread.store(false, Ordering::Relaxed);
                        return;
                    }
                };
                let result = rt.block_on(run_connection(url, to_net_rx, from_net_tx));
                if let Err(e) = result {
                    warn!("net connection ended: {e:#}");
                }
                conn_for_thread.store(false, Ordering::Relaxed);
            })
            .expect("failed to spawn net thread");

        Ok(NativeNet {
            to_net: to_net_tx,
            from_net: from_net_rx,
            connected,
            _thread: handle,
        })
    }
}

impl Net for NativeNet {
    fn try_recv(&mut self) -> Option<ServerMsg> {
        match self.from_net.try_recv() {
            Ok(msg) => Some(msg),
            Err(_) => None,
        }
    }

    fn send(&self, msg: &ClientMsg) {
        // Cheap clone; ClientMsg is small. The sender drops the message if
        // the runtime thread already exited.
        let _ = self.to_net.send(msg.clone());
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }
}

async fn run_connection(
    url: String,
    mut to_net_rx: mpsc::UnboundedReceiver<ClientMsg>,
    from_net_tx: std::sync::mpsc::Sender<ServerMsg>,
) -> Result<()> {
    info!(%url, "connecting");
    let (ws, _resp) = tokio_tungstenite::connect_async(&url).await?;
    info!(%url, "connected");
    let (mut ws_tx, mut ws_rx) = ws.split();

    // Writer task — drains outbound messages.
    let writer = tokio::spawn(async move {
        while let Some(msg) = to_net_rx.recv().await {
            let bytes = protocol::encode(&msg);
            if let Err(e) = ws_tx.send(Message::Binary(bytes)).await {
                warn!("ws write failed: {e}");
                break;
            }
        }
        let _ = ws_tx.close().await;
    });

    // Reader loop runs on this task.
    while let Some(frame) = ws_rx.next().await {
        let frame = frame?;
        match frame {
            Message::Binary(b) => {
                let sm: ServerMsg = match protocol::decode(&b) {
                    Ok(sm) => sm,
                    Err(e) => {
                        warn!("malformed server message: {e}");
                        continue;
                    }
                };
                if from_net_tx.send(sm).is_err() {
                    // main thread dropped the receiver; we're done.
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    writer.abort();
    Ok(())
}
