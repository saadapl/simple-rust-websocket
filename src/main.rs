use warp::Filter;
use warp::ws::{Message, WebSocket};
use tokio::sync::broadcast;
use futures_util::{StreamExt, SinkExt};
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let (tx, _rx) = broadcast::channel::<String>(10); // Shared message channel

    // WebSocket route
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || tx.clone())) // Pass the broadcast sender
        .map(|ws: warp::ws::Ws, tx| ws.on_upgrade(move |socket| handle_connection(socket, tx)));

    // Run the server on a different address (e.g., backend-server.com)
    let addr: SocketAddr = ([127, 0, 0, 1], 3030).into(); // Listen on all interfaces (0.0.0.0)
    println!("WebSocket Server running at ws://backend-server.com:3030");
    warp::serve(ws_route).run(addr).await;
}

async fn handle_connection(ws: WebSocket, tx: broadcast::Sender<String>) {
    let (mut tx_ws, mut rx_ws) = ws.split();
    let mut rx = tx.subscribe(); // Subscribe to broadcast messages

    // Task to receive messages from clients
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = rx_ws.next().await {
            if msg.is_text() {
                let text = msg.to_str().unwrap();
                println!("Received: {}", text); // Print on server
                let _ = tx.send(text.to_string()); // Broadcast to all clients
            }
        }
    });

    // Task to send updates to connected clients
    let send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if tx_ws.send(Message::text(msg)).await.is_err() {
                break;
            }
        }
    });

    let _ = tokio::join!(recv_task, send_task);
}