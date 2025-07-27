use reqwest::{Client, StatusCode};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use fastrand;

// --- Configuration ---
const SERVER_URL: &str = "http://127.0.0.1:8080/mcp";
const NUM_CLIENTS: usize = 300;
const TEST_DURATION_SECS: u64 = 30;

// --- Shared Statistics ---
// We use atomics for safe, lock-free counting across many threads.
#[derive(Debug, Default)]
struct SharedStats {
    successful_inits: AtomicUsize,
    successful_tool_calls: AtomicUsize,
    successful_prompt_gets: AtomicUsize,
    successful_resource_reads: AtomicUsize,
    failed_calls: AtomicUsize,
}

/// Simulates the entire lifecycle of a single MCP client performing various actions.
async fn run_client_simulation(client_id: usize, stats: Arc<SharedStats>) {
    let http_client = Client::new();

    // --- 1. Initialize Session ---
    let init_response = match http_client
        .post(SERVER_URL)
        .timeout(Duration::from_secs(10))
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
    stats.successful_inits.fetch_add(1, Ordering::Relaxed);

    let session_id = if let Some(header_value) = init_response.headers().get("Mcp-Session-Id") {
        if let Ok(id_str) = header_value.to_str() {
            id_str.to_string()
        } else {
            return;
        }
    } else {
        return;
    };

    // --- 2. Main Load Loop ---
    let start_time = Instant::now();
    let mut request_id_counter = 1;
    while start_time.elapsed() < Duration::from_secs(TEST_DURATION_SECS) {
        // Randomly pick an action to perform
        let action = fastrand::u8(0..4);
        let mut success = false;

        match action {
            // Action 0: Call a random tool
            0 => {
                let tool_to_call = if fastrand::bool() { "calculator" } else { "multiplier" };
                let operation = if tool_to_call == "calculator" { "add" } else { "multiply" };
                let res = http_client
                    .post(SERVER_URL)
                    .header("Mcp-Session-Id", &session_id)
                    .json(&json!({
                        "jsonrpc": "2.0", "id": request_id_counter, "method": "tools/call",
                        "params": {
                            "name": tool_to_call,
                            "arguments": { "operation": operation, "a": client_id, "b": request_id_counter }
                        }
                    }))
                    .send().await;
                if let Ok(resp) = res {
                    if resp.status() == StatusCode::OK {
                        stats.successful_tool_calls.fetch_add(1, Ordering::Relaxed);
                        success = true;
                    }
                }
            }
            // Action 1: Get a prompt
            1 => {
                let res = http_client
                    .post(SERVER_URL)
                    .header("Mcp-Session-Id", &session_id)
                    .json(&json!({
                        "jsonrpc": "2.0", "id": request_id_counter, "method": "prompts/get",
                        "params": { "name": "code_review", "arguments": { "code": "fn main() {}" } }
                    }))
                    .send().await;
                if let Ok(resp) = res {
                    if resp.status() == StatusCode::OK {
                        stats.successful_prompt_gets.fetch_add(1, Ordering::Relaxed);
                        success = true;
                    }
                }
            }
            // Action 2: Read a resource
            2 => {
                 let res = http_client
                    .post(SERVER_URL)
                    .header("Mcp-Session-Id", &session_id)
                    .json(&json!({
                        "jsonrpc": "2.0", "id": request_id_counter, "method": "resources/read",
                        "params": { "uri": "mcp://status/current" }
                    }))
                    .send().await;
                if let Ok(resp) = res {
                    if resp.status() == StatusCode::OK {
                        stats.successful_resource_reads.fetch_add(1, Ordering::Relaxed);
                        success = true;
                    }
                }
            }
            // Action 3: List something (tools, prompts, or resources)
            _ => {
                let list_method = match fastrand::u8(0..3) {
                    0 => "tools/list",
                    1 => "prompts/list",
                    _ => "resources/list",
                };
                 let res = http_client
                    .post(SERVER_URL)
                    .header("Mcp-Session-Id", &session_id)
                    .json(&json!({
                        "jsonrpc": "2.0", "id": request_id_counter, "method": list_method,
                    }))
                    .send().await;
                if let Ok(resp) = res {
                    if resp.status() == StatusCode::OK {
                        // We count lists as successful tool calls for simplicity
                        stats.successful_tool_calls.fetch_add(1, Ordering::Relaxed);
                        success = true;
                    }
                }
            }
        }

        if !success {
            stats.failed_calls.fetch_add(1, Ordering::Relaxed);
        }

        request_id_counter += 1;
        tokio::time::sleep(Duration::from_millis(fastrand::u64(500..1500))).await;
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("--- Starting Comprehensive MCP Server Load Test ---");
    println!("Make sure the server is running in a separate terminal!");
    println!("Simulating {} concurrent clients for {} seconds...", NUM_CLIENTS, TEST_DURATION_SECS);

    tokio::time::sleep(Duration::from_secs(3)).await;

    let stats = Arc::new(SharedStats::default());
    let mut tasks = JoinSet::new();
    for i in 0..NUM_CLIENTS {
        let client_stats = stats.clone();
        tasks.spawn(async move {
            run_client_simulation(i, client_stats).await;
        });
    }

    println!("All {} client tasks spawned. Test is running...", NUM_CLIENTS);

    while let Some(_) = tasks.join_next().await {}

    println!("\n--- Load Test Finished ---");
    println!("\n--- Results ---");
    println!("Successful initializations: {}", stats.successful_inits.load(Ordering::Relaxed));
    println!("Successful tool calls (incl. lists): {}", stats.successful_tool_calls.load(Ordering::Relaxed));
    println!("Successful prompt gets: {}", stats.successful_prompt_gets.load(Ordering::Relaxed));
    println!("Successful resource reads: {}", stats.successful_resource_reads.load(Ordering::Relaxed));
    println!("Total failed calls: {}", stats.failed_calls.load(Ordering::Relaxed));
    println!("-----------------");

    Ok(())
}
