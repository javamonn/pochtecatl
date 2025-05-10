# Pochtecatl

A Rust-based framework for developing, backtesting, and executing trading strategies on Ethereum-based decentralized exchanges.

## Architecture

```
                      ┌─────────────────┐
                      │  Ethereum Node  │
                      └────────┬────────┘
                               │
                               ▼
┌────────────────────────────────────────────────────┐
│                     Strategy                       │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────┐  │
│  │ Block Range  │  │   Strategy   │  │   Trade  │  │
│  │   Indexer    │─▶│   Executor   │─▶│Controller│  │
│  └──────────────┘  └──────────────┘  └──────────┘  │
└──────────────────────────┬──────────────────────┬──┘
                           │                      │
                           ▼                      ▼
                      ┌─────────┐           ┌─────────┐
                      │ Database│           │   API   │
                      └─────────┘           └─────────┘
                                                │
                                                ▼
                                          ┌──────────┐
                                          │  Client  │
                                          └──────────┘
```

## Components

### Core Libraries

#### Primitives (`crates/primitives`)
- Contains fundamental data structures for blockchain interaction
- Defines interfaces for DEX providers (Uniswap V2, V3)
- Implements time-price bars and technical indicators
- Provides RPC abstractions for blockchain communication

#### Database (`crates/db`)
- SQLite-based persistence layer
- Stores blocks, backtests, and trade information
- Manages database migrations and schema

### Binaries

#### Strategy Engine (`bin/strategy`)
- Indexes blockchain data from specified block ranges
- Executes trading strategies on historical or live data
- Implements the trade controller for managing positions
- Currently supports a momentum-based trading strategy
- Supports both Uniswap V2 and V3 DEXs

#### API Server (`bin/api`)
- HTTP server for querying backtest results
- Provides endpoints to list backtests and view trade details
- Built with Axum web framework

## Getting Started

### Prerequisites
- Rust toolchain (1.75+)
- Access to an Ethereum RPC endpoint

### Building
```bash
cargo build --release
```

### Running a Backtest
```bash
cargo run --bin strategy -- backtest --start-block <start_block> --end-block <end_block>
```

### Viewing Results
```bash
cargo run --bin api -- --port 3000
```
Then access `http://localhost:3000/backtests` in your browser.

## Extension Points

### Adding New Strategies
Implement the `Strategy` trait in `bin/strategy/src/strategies/strategy.rs`.

### Supporting New DEXs
Implement the `DexProvider` trait in `crates/primitives/src/rpc_provider/dex_provider.rs`.

