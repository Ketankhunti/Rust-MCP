use reqwest::{Client, StatusCode};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::task::JoinSet;

// --- Configuration ---
const SERVER_URL: &str = "http://127.0.0.1:8080/mcp";
const NUM_REQUESTS: usize = 500; // The number of simultaneous requests to send

// --- Shared Statistics ---
#[derive(Debug, Default)]
struct SharedStats {
    successful_calls: AtomicUsize,
    failed_calls: AtomicUsize,
}

/// Sends a single, concurrent request to the server.
async fn send_request(
    session_id: Arc<String>,
    request_id: usize,
    stats: Arc<SharedStats>,
    http_client: Client,
) {
    let res = http_client
        .post(SERVER_URL)
        .header("Mcp-Session-Id", &**session_id)
        .timeout(Duration::from_secs(20)) // Set a generous timeout for each request
        .json(&json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {
                "name": "calculator",
                "arguments": { "operation": "add", "a": request_id, "b": request_id }
            }
        }))
        .send()
        .await;

    match res {
        Ok(response) if response.status() == StatusCode::OK => {
            stats.successful_calls.fetch_add(1, Ordering::Relaxed);
        }
        _ => {
            stats.failed_calls.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("--- Starting Single-Client Burst Test ---");
    println!("Make sure the server is running with multiple worker threads!");
    println!("Sending {} simultaneous requests from a single client...", NUM_REQUESTS);

    let http_client = Client::new();
    let stats = Arc::new(SharedStats::default());

    // --- 1. Initialize a single session ---
    let init_response = http_client
        .post(SERVER_URL)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {} }
        }))
        .send()
        .await?;

    if init_response.status() != StatusCode::OK {
        println!("Failed to initialize session. Exiting.");
        return Ok(());
    }

    let session_id = Arc::new(
        init_response
            .headers()
            .get("Mcp-Session-Id")
            .unwrap()
            .to_str()?
            .to_string(),
    );
    println!("Session initialized: {}", session_id);

    // --- 2. Spawn all request tasks to run concurrently ---
    let mut tasks = JoinSet::new();
    for i in 1..=NUM_REQUESTS {
        tasks.spawn(send_request(
            session_id.clone(),
            i,
            stats.clone(),
            http_client.clone(),
        ));
    }

    println!("All {} request tasks spawned. Waiting for completion...", NUM_REQUESTS);

    // --- 3. Wait for all requests to finish ---
    while let Some(_) = tasks.join_next().await {}

    println!("\n--- Burst Test Finished ---");
    println!("\n--- Results ---");
    println!("Successful calls: {}", stats.successful_calls.load(Ordering::Relaxed));
    println!("Failed calls:     {}", stats.failed_calls.load(Ordering::Relaxed));
    println!("-----------------");

    Ok(())
}
