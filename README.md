# Rust Model Context Protocol (MCP) SDK

A Rust SDK for building high-performance, asynchronous servers and clients that implement the Model Context Protocol (MCP). MCP is designed for scalable, concurrent, and extensible model-serving infrastructure, supporting tools, resources, and prompt management.

## Asynchronous Architecture

This SDK leverages Rust's async ecosystem (`tokio`, `reqwest`, etc.) to provide non-blocking, highly concurrent server implementation. The MCP server can handle hundreds of simultaneous client connections, making it ideal for real-time model serving and interactive applications.

- **Async Server:** MCP servers are built using async Rust, allowing efficient handling of requests without blocking threads.
- **Multi-threaded Execution:** Example servers use multi-threaded async runtimes for maximum throughput.
- **Concurrent Clients:** Simulation tests demonstrate the server's ability to manage hundreds of concurrent clients performing protocol actions.

## Getting Started

### Prerequisites

- Rust (latest stable recommended)
- Cargo

### Installation

Clone the repository:

```powershell
git clone https://github.com/Ketankhunti/Rust-MCP.git
cd Rust-MCP
```

Build the project:

```powershell
cargo build --release
```

## Usage

### Running the Asynchronous MCP HTTP Server

Start the example async server:

```powershell
cargo run --example http_macro_tool_server
```

The server starts on `127.0.0.1:8080` and uses an async runtime to handle requests efficiently.

### Running Asynchronous Simulation Tests

#### Comprehensive Load Simulation

To test the server under heavy concurrent load:

```powershell
cargo run --example http_all_test_load_simulation
```

This launches 400 async clients, each simulating a full MCP session for 30 seconds. The test showcases the server’s ability to process many requests in parallel.

#### Single Client Load Test

For a simple async client simulation:

```powershell
cargo run --example single_client_load_test
```

## Examples

- `examples/http_macro_tool_server.rs`: Asynchronous MCP server with macro-based resource and tool registration.
- `examples/http_all_test_load_simulation.rs`: Async load test simulating many MCP clients.
- `examples/single_client_load_test.rs`: Simple async single-client simulation.

## Project Structure

- `src/`: Main SDK source code
- `examples/`: Example servers and simulation tests
- `mcp_macros/`: Macro definitions for resources and tools

## Contributing

Pull requests and issues are welcome!

## License

MIT