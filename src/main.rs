use ethers::prelude::{
    Http, LocalWallet, Middleware, MiddlewareBuilder, Provider, ProviderExt, Signer, StreamExt,
    TransactionRequest, Ws,
};
use eyre::Result;
use std::env;
use std::error::Error;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let provider = Provider::<Ws>::connect(env::var("RPC_WS_URL")?).await?;
    let provider_tx =
        Arc::new(Provider::<Http>::connect("https://arb1-sequencer.arbitrum.io/rpc").await);

    let wallet = env::var("PRIVATE_KEY")?
        .parse::<LocalWallet>()
        .expect("Could not parse private key")
        .with_chain_id(provider.get_chainid().await.unwrap().as_u64());

    let wallet_address = wallet.address();

    let provider_tx = provider_tx.with_signer(wallet);

    let nonce = provider.get_transaction_count(wallet_address, None).await?;
    let gas_price = provider.get_gas_price().await?;

    tokio::spawn(async move {
        let mut i = 0;
        let mut stream = provider.subscribe_blocks().await.unwrap();
        while let Some(block) = stream.next().await {
            i += 1;

            println!("New block: {}", block.number.unwrap());

            let provider_tx = provider_tx.clone();

            if (9..=10).contains(&i) {
                let nonce = nonce + i - 9;
                tokio::spawn(async move {
                    let now = std::time::Instant::now();

                    let tx = provider_tx
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

                    println!(
                        "{} @ {} in {:?}",
                        tx.tx_hash(),
                        block.number.unwrap(),
                        now.elapsed()
                    );
                });
            }

            if i == 30 {
                break;
            }
        }
    })
    .await?;

    Ok(())
}
