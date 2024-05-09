use ethers::prelude::{
    Http, LocalWallet, Middleware, MiddlewareBuilder, Provider, ProviderExt, Signer, StreamExt,
    TransactionRequest, Ws,
};
use eyre::Result;
use std::env;
use std::error::Error;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::task::JoinSet;

// Constants for transaction settings
const TOTAL_TX: u64 = 3;
const SEND_TX_EVERY: u64 = 10;
const SEQUENCER_URL: &str = "https://arb1-sequencer.arbitrum.io/rpc";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let provider = Provider::<Ws>::connect(env::var("RPC_WS_URL")?).await?;
    let provider_tx = Arc::new(Provider::<Http>::connect(SEQUENCER_URL).await);

    let wallet = env::var("PRIVATE_KEY")?
        .parse::<LocalWallet>()
        .expect("Could not parse private key")
        .with_chain_id(provider.get_chainid().await.unwrap().as_u64());

    let wallet_address = wallet.address();

    let provider_tx = provider_tx.with_signer(wallet);

    let nonce = AtomicU64::new(
        provider
            .get_transaction_count(wallet_address, None)
            .await?
            .as_u64(),
    );
    let gas_price = provider.get_gas_price().await?;

    let tx_durations = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let mut join_set = JoinSet::new();
    let mut i = 0;

    let mut stream = provider.subscribe_blocks().await.unwrap();
    while let Some(block) = stream.next().await {
        if block.number.unwrap().as_u64() % SEND_TX_EVERY == 0 && i < TOTAL_TX {
            let nonce = nonce.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let provider_tx = provider_tx.clone();
            let tx_durations = tx_durations.clone();

            join_set.spawn(async move {
                let now = std::time::Instant::now();

                provider_tx
                    .send_transaction(
                        TransactionRequest::new()
                            .to(wallet_address)
                            .value(0)
                            .gas(50000)
                            .gas_price(gas_price)
                            .nonce(nonce)
                            .data(vec![]),
                        None,
                    )
                    .await
                    .unwrap();

                let sent_in = now.elapsed();
                tx_durations.lock().await.push(sent_in);

                println!("{},{:?}", block.number.unwrap(), sent_in);
            });

            i += 1;
        }

        if i == TOTAL_TX {
            break;
        }
    }

    // Wait for all transactions to be sent
    while join_set.join_next().await.is_some() {}

    print_stats(tx_durations.lock().await.clone());

    Ok(())
}

#[derive(Debug)]
struct Stats {
    total_tx: u64,
    mean: std::time::Duration,
    median: std::time::Duration,
    min: std::time::Duration,
    max: std::time::Duration,
}

fn print_stats(durations: Vec<std::time::Duration>) {
    // mean
    let sum: std::time::Duration = durations.iter().sum();
    let mean = sum / durations.len() as u32;

    // median
    let mut tx_durations = durations.clone();
    tx_durations.sort();
    let median = tx_durations[tx_durations.len() / 2];

    // min-max
    let min = tx_durations.iter().min().unwrap();
    let max = tx_durations.iter().max().unwrap();

    let stats = Stats {
        total_tx: durations.len() as u64,
        mean,
        median,
        min: *min,
        max: *max,
    };

    println!("{:#?}", stats);
}
