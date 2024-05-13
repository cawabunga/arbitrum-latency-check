use ethers::prelude::{
    Http, Ipc, LocalWallet, Middleware, MiddlewareBuilder, Provider, ProviderExt, Signer,
    StreamExt, TransactionRequest, Ws,
};
use eyre::Result;
use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::task::JoinSet;

// Constants for transaction settings
const SEND_TX_EACH: u64 = 10;
const SEQUENCER_URL: &str = "https://arb1-sequencer.arbitrum.io/rpc";

#[derive(Default)]
struct BlockTimes {
    subscribe_times: HashMap<u64, Instant>,
    poll_times: HashMap<u64, Instant>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let id = env::var("INSTANCE_ID").unwrap_or("0".to_string());
    let sequencer_url = env::var("SEQUENCER_URL").unwrap_or(SEQUENCER_URL.to_string());
    let tx_total = env::var("TX_TOTAL")
        .unwrap_or("10".to_string())
        .parse::<u64>()?;
    let tx_each = env::var("TX_EACH")
        .unwrap_or(SEND_TX_EACH.to_string())
        .parse::<u64>()?;

    let provider = setup_ipc_provider().await?;

    let provider_tx = Arc::new(Provider::<Http>::connect(sequencer_url.as_str()).await);

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
    let i = Arc::new(AtomicU64::new(0));

    let (new_block_tx, mut new_block_rx) = tokio::sync::mpsc::channel::<(u64, &str, Instant)>(1);
    let block_times = Arc::new(Mutex::new(BlockTimes::default()));

    let provider_clone = provider.clone();
    let new_block_tx_clone = new_block_tx.clone();
    let i_clone = i.clone();
    let block_times_clone = block_times.clone();
    join_set.spawn(async move {
        let mut stream = provider_clone.subscribe_blocks().await.unwrap();
        while let Some(block) = stream.next().await {
            let block_number = block.number.unwrap().as_u64();

            block_times_clone
                .lock()
                .unwrap()
                .subscribe_times
                .entry(block_number)
                .or_insert(Instant::now());

            new_block_tx_clone
                .send((block_number, "subscribe", Instant::now()))
                .await
                .unwrap();

            if i_clone.load(Ordering::SeqCst) == tx_total {
                drop(new_block_tx_clone);
                break;
            }
        }
    });

    let provider_clone = provider.clone();
    let new_block_tx_clone = new_block_tx.clone();
    let i_clone = i.clone();
    let block_times_clone = block_times.clone();
    join_set.spawn(async move {
        // just poll for new blocks instead of subscribing
        let mut last_block = provider_clone.get_block_number().await.unwrap().as_u64();
        loop {
            let current_block = provider_clone.get_block_number().await.unwrap().as_u64();
            if current_block > last_block {
                block_times_clone
                    .lock()
                    .unwrap()
                    .poll_times
                    .entry(current_block)
                    .or_insert(Instant::now());

                new_block_tx
                    .send((current_block, "poll", Instant::now()))
                    .await
                    .unwrap();
                last_block = current_block;
            }

            if i_clone.load(Ordering::SeqCst) == tx_total {
                drop(new_block_tx_clone);
                break;
            }
        }
    });

    let mut processed_blocks = HashSet::new();

    while let Some((block_number, initiator, _)) = new_block_rx.recv().await {
        let times = block_times.lock().unwrap();
        if times.subscribe_times.contains_key(&block_number)
            && times.poll_times.contains_key(&block_number)
        {
            let subscribe_time = times.subscribe_times[&block_number];
            let poll_time = times.poll_times[&block_number];
            let diff = poll_time - subscribe_time;
            println!("{},{},{},{:?}", id, block_number, initiator, diff);
        }
        drop(times);

        if processed_blocks.contains(&block_number) {
            continue;
        }
        processed_blocks.insert(block_number);

        if block_number % tx_each == 0 && i.load(Ordering::SeqCst) < tx_total {
            let nonce = nonce.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let provider_tx = provider_tx.clone();
            let tx_durations = tx_durations.clone();
            let id = id.clone();

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

                println!("{},{},{:?}", id, block_number, sent_in);
            });

            i.fetch_add(1, Ordering::SeqCst);
        }
    }

    // Wait for all transactions to be sent
    while join_set.join_next().await.is_some() {}

    print_stats(tx_durations.lock().await.clone(), id);

    Ok(())
}

#[allow(dead_code)]
async fn setup_ws_provider() -> Result<Provider<Ws>, Box<dyn Error>> {
    let url = env::var("RPC_WS_URL")?;
    Ok(Provider::<Ws>::connect(url).await?)
}

#[allow(dead_code)]
async fn setup_ipc_provider() -> Result<Provider<Ipc>, Box<dyn Error>> {
    let path = env::var("RPC_IPC_PATH")?;
    Ok(Provider::<Ipc>::connect_ipc(path).await?)
}

#[derive(Debug)]
#[allow(dead_code)]
struct Stats {
    id: String,
    total_tx: u64,
    mean: std::time::Duration,
    median: std::time::Duration,
    min: std::time::Duration,
    max: std::time::Duration,
}

fn print_stats(durations: Vec<std::time::Duration>, id: String) {
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
        id,
        total_tx: durations.len() as u64,
        mean,
        median,
        min: *min,
        max: *max,
    };

    println!("{:#?}", stats);
}
