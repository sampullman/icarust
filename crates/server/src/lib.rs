//! Icarust server. Runs the authoritative `sim::World` at 60 Hz, accepts
//! WebSocket connections, broadcasts snapshots at 20 Hz and game events as
//! they fire.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::{self, Duration, MissedTickBehavior};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use protocol::{snapshot_from_world, ClientMsg, ServerMsg, Snapshot};
use sim::{GameEvent, PlayerId, PlayerInput, PlayerInputs, Vec2, World, WorldConfig, TICK_DT};

/// Send a snapshot every Nth tick. 60 Hz / 3 = 20 Hz.
const SNAPSHOT_EVERY: u64 = 3;
/// Broadcast channel capacity per receiver. Tuned so a brief stall on one
/// client does not lag the rest.
const BROADCAST_CAP: usize = 256;

#[derive(Debug)]
enum Command {
    Join {
        player_id: PlayerId,
        reply: oneshot::Sender<JoinAck>,
    },
    Leave(PlayerId),
    Input(PlayerId, PlayerInput),
    Respawn(PlayerId),
}

#[derive(Debug, Clone)]
struct JoinAck {
    snapshot: Snapshot,
    seed: u64,
    world_size: Vec2,
}

/// Run the server using a pre-bound listener. Useful from tests that bind
/// to port 0 to grab a free port.
pub async fn run_with_listener(listener: TcpListener) -> Result<()> {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Command>();
    let (out_tx, _) = broadcast::channel::<Arc<ServerMsg>>(BROADCAST_CAP);

    tokio::spawn(game_loop(cmd_rx, out_tx.clone()));

    let next_pid = Arc::new(AtomicU32::new(1));

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(p) => p,
            Err(e) => {
                warn!("accept failed: {e}");
                continue;
            }
        };
        let pid = PlayerId(next_pid.fetch_add(1, Ordering::Relaxed));
        let cmd_tx = cmd_tx.clone();
        let out_rx = out_tx.subscribe();
        tokio::spawn(async move {
            let cmd_for_leave = cmd_tx.clone();
            if let Err(e) = serve_connection(stream, peer.to_string(), pid, cmd_tx, out_rx).await {
                warn!(?pid, "connection ended: {e:#}");
            }
            let _ = cmd_for_leave.send(Command::Leave(pid));
        });
    }
}

/// Bind on `addr` and run forever.
#[allow(dead_code)] // exercised from integration tests via `main`-level entry
pub async fn run(addr: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    run_with_listener(listener).await
}

async fn serve_connection(
    stream: TcpStream,
    peer: String,
    pid: PlayerId,
    cmd_tx: mpsc::UnboundedSender<Command>,
    mut out_rx: broadcast::Receiver<Arc<ServerMsg>>,
) -> Result<()> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut ws_tx, mut ws_rx) = ws.split();

    // First message must be Hello.
    let hello_frame = ws_rx
        .next()
        .await
        .ok_or_else(|| anyhow!("client closed before Hello"))??;
    let bytes = match hello_frame {
        Message::Binary(b) => b,
        Message::Text(t) => t.into_bytes(),
        Message::Close(_) => return Ok(()),
        other => return Err(anyhow!("unexpected first frame: {other:?}")),
    };
    let name = match protocol::decode::<ClientMsg>(&bytes)? {
        ClientMsg::Hello { name } => name,
        other => return Err(anyhow!("first message must be Hello, got {other:?}")),
    };

    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .send(Command::Join {
            player_id: pid,
            reply: reply_tx,
        })
        .map_err(|_| anyhow!("game loop dropped"))?;
    let ack = reply_rx.await?;

    info!(?pid, %peer, %name, "player joined");

    let welcome = ServerMsg::Welcome {
        player_id: pid,
        seed: ack.seed,
        world_size: ack.world_size.into(),
        snapshot: ack.snapshot,
    };
    ws_tx.send(Message::Binary(protocol::encode(&welcome))).await?;

    // Writer task — drains incoming broadcasts and writes to the socket.
    let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Arc<ServerMsg>>();
    let write_pid = pid;
    let writer = tokio::spawn(async move {
        while let Some(msg) = write_rx.recv().await {
            let bytes = protocol::encode(&*msg);
            if let Err(e) = ws_tx.send(Message::Binary(bytes)).await {
                warn!(?write_pid, "ws write failed: {e}");
                break;
            }
        }
        let _ = ws_tx.close().await;
    });

    // Bridge broadcast → writer mpsc. Lets us drop subscribers cleanly when
    // the writer exits (no orphaned broadcast slots).
    let bridge_tx = write_tx.clone();
    let bridge = tokio::spawn(async move {
        loop {
            match out_rx.recv().await {
                Ok(m) => {
                    if bridge_tx.send(m).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(?pid, "broadcast lagged by {n}");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Reader loop.
    while let Some(frame) = ws_rx.next().await {
        let frame = frame?;
        match frame {
            Message::Binary(b) => {
                let cm: ClientMsg = match protocol::decode(&b) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!(?pid, "malformed client message: {e}");
                        continue;
                    }
                };
                match cm {
                    ClientMsg::Input { input, .. } => {
                        let _ = cmd_tx.send(Command::Input(pid, input));
                    }
                    ClientMsg::Respawn => {
                        let _ = cmd_tx.send(Command::Respawn(pid));
                    }
                    ClientMsg::Bye => break,
                    ClientMsg::Hello { .. } => {} // ignore re-hello
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    drop(write_tx);
    bridge.abort();
    let _ = writer.await;
    Ok(())
}

async fn game_loop(
    mut cmd_rx: mpsc::UnboundedReceiver<Command>,
    out_tx: broadcast::Sender<Arc<ServerMsg>>,
) {
    let config = WorldConfig::default();
    let mut world = World::new(config);
    let world_size = world.world_size();
    let seed = config.seed;
    let mut current_inputs: PlayerInputs = PlayerInputs::new();

    let mut interval = time::interval(Duration::from_secs_f32(TICK_DT));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        interval.tick().await;

        // Drain commands without blocking.
        loop {
            match cmd_rx.try_recv() {
                Ok(Command::Join { player_id, reply }) => {
                    world.add_player(player_id);
                    let snap = snapshot_from_world(&world);
                    let _ = reply.send(JoinAck {
                        snapshot: snap,
                        seed,
                        world_size,
                    });
                    let msg = Arc::new(ServerMsg::Events {
                        tick: world.tick_index(),
                        events: vec![GameEvent::PlayerJoined(player_id)],
                    });
                    let _ = out_tx.send(msg);
                }
                Ok(Command::Leave(pid)) => {
                    if !world.has_player(pid) {
                        continue;
                    }
                    world.remove_player(pid);
                    current_inputs.remove(&pid);
                    let msg = Arc::new(ServerMsg::Events {
                        tick: world.tick_index(),
                        events: vec![GameEvent::PlayerLeft(pid)],
                    });
                    let _ = out_tx.send(msg);
                }
                Ok(Command::Input(pid, input)) => {
                    current_inputs.insert(pid, input);
                }
                Ok(Command::Respawn(pid)) => {
                    if world.add_player(pid).is_some() {
                        // Drop any held input from before death so the
                        // respawned ship doesn't immediately fly off.
                        current_inputs.remove(&pid);
                        let msg = Arc::new(ServerMsg::Events {
                            tick: world.tick_index(),
                            events: vec![GameEvent::PlayerJoined(pid)],
                        });
                        let _ = out_tx.send(msg);
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => return,
            }
        }

        let events = world.tick(&current_inputs, TICK_DT);
        let tick = world.tick_index();
        if !events.is_empty() {
            let _ = out_tx.send(Arc::new(ServerMsg::Events { tick, events }));
        }
        if tick.0 % SNAPSHOT_EVERY == 0 {
            let snap = snapshot_from_world(&world);
            let _ = out_tx.send(Arc::new(ServerMsg::Snapshot(snap)));
        }
    }
}
