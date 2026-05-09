//! End-to-end smoke test: stand up the server on an ephemeral port,
//! drive a fake client through the WebSocket, and assert the protocol
//! contract holds.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMsg, ServerMsg};
use sim::entity::ShotOwner;
use sim::{PlayerInput, Tick};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn hello_yields_welcome_and_snapshots() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");

    let server = tokio::spawn(async move {
        let _ = server::run_with_listener(listener).await;
    });

    let (mut ws, _) = timeout(Duration::from_secs(2), tokio_tungstenite::connect_async(&url))
        .await
        .expect("connect timed out")
        .expect("connect failed");

    // Hello.
    ws.send(Message::Binary(protocol::encode(&ClientMsg::Hello {
        name: "tester".into(),
    })))
    .await
    .unwrap();

    // Welcome.
    let frame = timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("welcome timed out")
        .expect("welcome stream end")
        .expect("welcome ws err");
    let bytes = match frame {
        Message::Binary(b) => b,
        other => panic!("expected binary, got {other:?}"),
    };
    let msg: ServerMsg = protocol::decode(&bytes).unwrap();
    let local_pid = match msg {
        ServerMsg::Welcome {
            player_id, snapshot, ..
        } => {
            assert!(!snapshot.entities.is_empty(), "welcome snapshot should include rocks");
            player_id
        }
        other => panic!("expected Welcome, got {other:?}"),
    };

    // Send a Respawn first. The player is already alive, so the server
    // should silently no-op it — but it must still parse and consume the
    // message rather than dropping the connection. If the variant ever
    // gets misordered or a field is added wrong, this catches it.
    ws.send(Message::Binary(protocol::encode(&ClientMsg::Respawn)))
        .await
        .unwrap();

    // Send a fire input. Expect to see a ShotFired event eventually.
    ws.send(Message::Binary(protocol::encode(&ClientMsg::Input {
        tick: Tick(1),
        input: PlayerInput {
            xaxis: 0.0,
            yaxis: 0.0,
            fire: true,
        },
    })))
    .await
    .unwrap();

    let mut saw_shot = false;
    let mut saw_snapshot_with_player = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline && !(saw_shot && saw_snapshot_with_player) {
        let frame = match timeout(Duration::from_millis(200), ws.next()).await {
            Ok(Some(Ok(f))) => f,
            _ => continue,
        };
        let bytes = match frame {
            Message::Binary(b) => b,
            _ => continue,
        };
        let msg: ServerMsg = match protocol::decode(&bytes) {
            Ok(m) => m,
            Err(_) => continue,
        };
        match msg {
            ServerMsg::Events { events, .. } => {
                if events
                    .iter()
                    .any(|e| matches!(e, sim::GameEvent::ShotFired { owner: ShotOwner::Player(pid), .. } if *pid == local_pid))
                {
                    saw_shot = true;
                }
            }
            ServerMsg::Snapshot(s) => {
                if s.entities.iter().any(|e| matches!(e.kind, sim::entity::EntityKind::Player { player_id } if player_id == local_pid)) {
                    saw_snapshot_with_player = true;
                }
            }
            ServerMsg::Welcome { .. } => {}
        }
    }

    assert!(saw_shot, "expected a ShotFired event for our fire input");
    assert!(saw_snapshot_with_player, "expected a snapshot containing our player entity");

    // Be a polite client.
    let _ = ws.close(None).await;
    server.abort();
}
