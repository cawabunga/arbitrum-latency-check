# Pre-requisites

- Rust

# How to run

```shell
# Build the program
cargo build --release

# Run the program
PRIVATE_KEY=0xYOUR_KEY RPC_WS_URL=/tmp/node.ipc ./target/release/arbitrum-latency-check-rs
```

Other environment variables that can be set:

| Variable        | Description                | Required | Default                                  |
|-----------------|----------------------------|----------|------------------------------------------|
| `PRIVATE_KEY`   | Private key of the account | Yes      |
| `RPC_WS_URL`    | WebSocket URL of RPC       | Yes      |
| `SEQUENCER_URL` | URL of the Sequencer RPC   |          | `https://arb1-sequencer.arbitrum.io/rpc` 
| `TOTAL_TX`      | Total transactions to send |          | `10`                                     
