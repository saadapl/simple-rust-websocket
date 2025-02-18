use warp::Filter;
use warp::ws::{Message, WebSocket};
use tokio::sync::broadcast;
use std::sync::{Arc, Mutex};
use rusqlite::{params, Connection};
use futures::{StreamExt, SinkExt};

#[tokio::main]
async fn main() {
    // Initialize database
    let db = Arc::new(Mutex::new(init_db()));

    // Create a broadcast channel for WebSocket messages
    let (tx, _rx) = broadcast::channel::<String>(100);

    // WebSocket endpoint
    let ws_route = warp::path("ws")
        .and(warp::ws())
        .and(with_db(db.clone()))
        .and(with_tx(tx.clone()))
        .map(|ws: warp::ws::Ws, db, tx| ws.on_upgrade(move |socket| handle_client(socket, db, tx)));

    // Serve static files (Frontend)
    let static_files = warp::fs::dir("./frontend");

    let routes = static_files.or(ws_route);

    println!("Server running on http://0.0.0.0:3030");
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}

// Middleware to pass database connection
fn with_db(db: Arc<Mutex<Connection>>) -> impl Filter<Extract = (Arc<Mutex<Connection>>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || db.clone())
}

// Middleware to pass WebSocket broadcast sender
fn with_tx(tx: broadcast::Sender<String>) -> impl Filter<Extract = (broadcast::Sender<String>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || tx.clone())
}

// Initialize SQLite database
fn init_db() -> Connection {
    let conn = Connection::open("data.db").expect("Failed to open database");

    conn.execute(
        "CREATE TABLE IF NOT EXISTS slider (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            value INTEGER NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    ).expect("Failed to create slider table");

    conn.execute(
        "CREATE TABLE IF NOT EXISTS clients (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ip TEXT NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    ).expect("Failed to create clients table");

    conn
}

// WebSocket connection handler
async fn handle_client(ws: WebSocket, db: Arc<Mutex<Connection>>, tx: broadcast::Sender<String>) {
    let (mut sender, mut receiver) = ws.split();
    let mut rx = tx.subscribe();

    // Log client connection
    let ip = "unknown"; // You can replace this with actual client IP if available
    let db_clone = db.clone();
    tokio::spawn(async move {
        let conn = db_clone.lock().unwrap();
        conn.execute("INSERT INTO clients (ip) VALUES (?1)", params![ip]).expect("Failed to insert client");
    });

    // Send the last stored slider value to the new client
    let last_value = {
        let conn = db.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM slider ORDER BY timestamp DESC LIMIT 1").unwrap();
        stmt.query_row([], |row| row.get::<_, i32>(0)).unwrap_or(50) // Default to 50 if empty
    };
    let _ = sender.send(Message::text(last_value.to_string())).await;

    // Handle incoming messages (slider updates)
    let tx_clone = tx.clone();
    let db_clone = db.clone();
    tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Ok(value) = msg.to_str().unwrap_or("50").parse::<i32>() {
                // Store slider value in the database
                let conn = db_clone.lock().unwrap();
                conn.execute("INSERT INTO slider (value) VALUES (?1)", params![value]).expect("Failed to insert slider value");

                // Broadcast new value to all clients
                let _ = tx_clone.send(value.to_string());
            }
        }
    });

    // Broadcast received messages to all clients
    while let Ok(msg) = rx.recv().await {
        let _ = sender.send(Message::text(msg)).await;
    }
}