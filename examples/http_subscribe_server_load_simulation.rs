use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use futures::StreamExt;
use fastrand; // <-- Import fastrand

// --- Configuration ---
const SERVER_URL: &str = "http://127.0.0.1:8080/mcp";
const NUM_CLIENTS: usize = 1000; // The number of concurrent clients to simulate
const TEST_DURATION_SECS: u64 = 30; // How long the test should run
const RESOURCE_URI_TO_SUBSCRIBE: &str = "mcp://server/visitors";

// --- Shared Statistics ---
// We use atomics for safe, lock-free counting across many threads.
#[derive(Debug, Default)]
struct SharedStats {
    successful_calls: AtomicUsize,
    failed_calls: AtomicUsize,
    notifications_received: AtomicUsize,
}

/// Simulates the entire lifecycle of a single MCP client.
async fn run_client_simulation(client_id: usize, stats: Arc<SharedStats>) {
    let http_client = Client::new();

    // --- 1. Initialize Session ---
    let init_response = match http_client
        .post(SERVER_URL)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {} }
        }))
        .send()
        .await
    {
        Ok(res) => res,
        Err(_) => {
            stats.failed_calls.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };

    if init_response.status() != StatusCode::OK {
        stats.failed_calls.fetch_add(1, Ordering::Relaxed);
        return;
    }

    let session_id = init_response
        .headers()
        .get("Mcp-Session-Id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // --- 2. Start SSE Listener in the background ---
    let sse_stats = stats.clone();
    let client_clone = http_client.clone();
    let clone_session_id = session_id.clone();
    tokio::spawn(async move {
        let mut stream = match client_clone.get(SERVER_URL).header("Mcp-Session-Id", &clone_session_id).send().await {
            Ok(res) => res.bytes_stream(),
            Err(_) => return,
        };

        while let Some(item) = stream.next().await {
            if let Ok(chunk) = item {
                let line = String::from_utf8_lossy(&chunk);
                if line.contains("notifications/resources/updated") {
                    sse_stats.notifications_received.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });

    // --- 3. Subscribe to the resource ---
    let _ = http_client
        .post(SERVER_URL)
        .header("Mcp-Session-Id", &session_id)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "resources/subscribe",
            "params": { "uri": RESOURCE_URI_TO_SUBSCRIBE }
        }))
        .send()
        .await;

    // --- 4. Main Load Loop ---
    // Each client will make requests for the duration of the test.
    let start_time = Instant::now();
    let mut request_id_counter = 2;
    while start_time.elapsed() < Duration::from_secs(TEST_DURATION_SECS) {
        let res = http_client
            .post(SERVER_URL)
            .header("Mcp-Session-Id", &session_id)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": request_id_counter,
                "method": "tools/call",
                "params": {
                    "name": "calculator",
                    "arguments": { "operation": "add", "a": client_id, "b": request_id_counter }
                }
            }))
            .send()
            .await;

        if let Ok(response) = res {
            if response.status() == StatusCode::OK {
                stats.successful_calls.fetch_add(1, Ordering::Relaxed);
            } else {
                stats.failed_calls.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            stats.failed_calls.fetch_add(1, Ordering::Relaxed);
        }
        request_id_counter += 1;
        // Wait for a short, random duration to simulate more realistic client behavior
        tokio::time::sleep(Duration::from_millis(fastrand::u64(500..2000))).await;
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("--- Starting MCP Server Load Test ---");
    println!("Make sure the server is running in a separate terminal!");
    println!("Simulating {} concurrent clients for {} seconds...", NUM_CLIENTS, TEST_DURATION_SECS);

    // Give yourself a moment to ensure the server is running.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // 2. Spawn all client simulations
    let stats = Arc::new(SharedStats::default());
    let mut tasks = JoinSet::new();
    for i in 0..NUM_CLIENTS {
        let client_stats = stats.clone();
        tasks.spawn(async move {
            run_client_simulation(i, client_stats).await;
        });
    }

    println!("All {} client tasks spawned. Test is running...", NUM_CLIENTS);

    // 3. Wait for all client tasks to complete
    while let Some(_) = tasks.join_next().await {}

    println!("\n--- Load Test Finished ---");

    // 4. Print results
    println!("\n--- Results ---");
    println!("Successful tool calls: {}", stats.successful_calls.load(Ordering::Relaxed));
    println!("Failed calls: {}", stats.failed_calls.load(Ordering::Relaxed));
    println!("Resource update notifications received (across all clients): {}", stats.notifications_received.load(Ordering::Relaxed));
    println!("-----------------");

    Ok(())
}
