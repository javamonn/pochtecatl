use crate::strategies::StrategyExecutor;

#[cfg(test)]
use crate::config;

use alloy::{network::Ethereum, providers::Provider, transports::Transport};
use eyre::Result;

pub trait Indexer<T, P>
where
    T: Transport + Clone,
    P: Provider<T, Ethereum> + 'static,
{
    async fn exec(&mut self, strategy_executor: StrategyExecutor<T, P>) -> Result<()>;
}
