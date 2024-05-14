use crate::config;

use super::{BlockProvider, TTLCache, IndexedTradeProvider};

use alloy::{
    network::{Ethereum, EthereumSigner},
    primitives::{Address, TxHash},
    providers::{
        layers::{GasEstimatorProvider, ManagedNonceProvider, SignerProvider},
        PendingTransactionBuilder, Provider, ProviderBuilder, RootProvider,
    },
    rpc::types::{
        eth::{BlockId, Filter, Header, Log, TransactionReceipt, TransactionRequest},
        trace::parity::{TraceResults, TraceType},
    },
    signers::wallet::LocalWallet,
    transports::{http::Http, Transport, TransportResult},
};

use eyre::{eyre, Result, WrapErr};
use std::sync::Arc;
use tracing::instrument;

pub struct RpcProvider<T: Transport + Clone, P: Provider<T, Ethereum>> {
    signer_address: Address,
    inner: Arc<P>,

    indexed_trade_provider: IndexedTradeProvider<T, P>,
    block_provider: BlockProvider<T, P>,
}

pub async fn new_http_signer_provider(
    url: &String,
    finalized_block_header_cache: Option<TTLCache<Header>>,
) -> Result<
    RpcProvider<
        Http<reqwest::Client>,
        SignerProvider<
            Http<reqwest::Client>,
            GasEstimatorProvider<
                Http<reqwest::Client>,
                ManagedNonceProvider<Http<reqwest::Client>, RootProvider<Http<reqwest::Client>>>,
                Ethereum,
            >,
            EthereumSigner,
        >,
    >,
> {
    let signer = LocalWallet::from_bytes(&config::WALLET_PRIVATE_KEY)?;
    let signer_address = signer.address();
    let inner = Arc::new(
        ProviderBuilder::new()
            .signer(EthereumSigner::from(signer))
            .with_gas_estimation()
            .with_nonce_management()
            .on_reqwest_http(url::Url::parse(url.as_str())?)
            .map_err(|err| eyre!("Failed to create provider: {:?}", err))?,
    );

    let indexed_trade_provider = IndexedTradeProvider::new(Arc::clone(&inner));
    let block_provider = BlockProvider::new(Arc::clone(&inner), finalized_block_header_cache);

    Ok(RpcProvider {
        inner,
        signer_address,
        indexed_trade_provider,
        block_provider,
    })
}

impl<T, P> RpcProvider<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    pub fn block_provider(&self) -> &BlockProvider<T, P> {
        &self.block_provider
    }

    pub fn indexed_trade_provider(&self) -> &IndexedTradeProvider<T, P> {
        &self.indexed_trade_provider
    }

    // TODO: used by disabled trace code
    #[allow(dead_code)]
    pub async fn trace_call_many(
        &self,
        tx_requests: &[(TransactionRequest, Vec<TraceType>)],
        block_id: Option<BlockId>,
    ) -> Result<TraceResults> {
        self.inner
            .trace_call_many(tx_requests, block_id)
            .await
            .wrap_err("trace_call_many failed")
    }

    // eth api
    pub async fn send_transaction(
        &self,
        tx_request: TransactionRequest,
    ) -> Result<PendingTransactionBuilder<'_, T, Ethereum>> {
        self.inner
            .send_transaction(tx_request)
            .await
            .wrap_err("send_transaction failed")
    }

    #[instrument(skip(self))]
    pub async fn get_logs(&self, filter: &Filter) -> TransportResult<Vec<Log>> {
        self.inner.get_logs(filter).await
    }

    pub async fn get_transaction_receipt(
        &self,
        hash: TxHash,
    ) -> Result<Option<TransactionReceipt>> {
        self.inner
            .get_transaction_receipt(hash)
            .await
            .wrap_err(format!("get_transaction_receipt {} failed", hash))
    }

    // custom api
    pub fn signer_address(&self) -> &Address {
        &self.signer_address
    }

    pub fn inner(&self) -> &P {
        &self.inner
    }
}
