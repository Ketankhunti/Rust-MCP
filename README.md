# Rust Model Context Protocol (MCP) SDK

A Rust SDK for building servers and clients that implement the Model Context Protocol (MCP). MCP is designed for scalable, concurrent, and extensible model-serving infrastructure, supporting tools, resources, and prompt management.

## Features

- MCP server implementation with HTTP transport
- Macro-based registration for resources, tools, and prompts
- Example servers and load simulation tests
- Support for concurrent clients and multi-threaded execution

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

### Running the Example MCP HTTP Server

Start the example server:

```powershell
cargo run --example http_macro_tool_server
```

The server will start on `127.0.0.1:8080`. It exposes MCP endpoints for tools, resources, and prompts.

### Running Simulation Tests

#### Comprehensive Load Simulation

To run a load simulation with many concurrent MCP clients:

```powershell
cargo run --example http_all_test_load_simulation
```

This simulates 450 concurrent clients performing MCP actions for 30 seconds. Ensure the MCP server is running in a separate terminal before starting the simulation.

#### Single Client Load Test

To run a single client simulation:

```powershell
cargo run --example single_client_load_test
```

## Examples

- `examples/http_macro_tool_server.rs`: Example MCP server with macro-based resource and tool registration.
- `examples/http_all_test_load_simulation.rs`: Comprehensive load test simulating many MCP clients.
- `examples/single_client_load_test.rs`: Simple single-client simulation.

## Project Structure

- `src/`: Main SDK source code
- `examples/`: Example servers and simulation tests
- `mcp_macros/`: Macro definitions for resources and tools

## Contributing

Pull requests and issues are welcome!

## License

MIT