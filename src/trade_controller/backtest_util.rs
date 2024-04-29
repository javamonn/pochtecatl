use alloy::{
    primitives::{BlockNumber, U256},
    rpc::types::eth::{Block, BlockTransactions},
};

use eyre::{eyre, Result};

use crate::rpc_provider::RpcProvider;

// Gets the effective_gas_price of the tx in the middle of the block we would've confirmed in
// while backtesting, and multiples it by the gas used to get the estimated gas fee.
pub async fn estimate_gas_fee(
    rpc_provider: &RpcProvider,
    gas: U256,
    block_number: BlockNumber,
) -> Result<U256> {
    // The effective gas price paid for the tx in the middle of the block we would've
    // confirmed in.
    match rpc_provider.get_block(block_number).await? {
        Some(Block {
            transactions: BlockTransactions::Hashes(hashes),
            ..
        }) => match hashes.get(hashes.len() / 2) {
            Some(hash) => match rpc_provider.get_transaction_receipt(hash.clone()).await? {
                Some(receipt) => Ok(U256::from(receipt.effective_gas_price) * gas),
                None => Err(eyre!("tx receipt {} not found", hash)),
            },
            None => Err(eyre!("no transactions in block {}", block_number)),
        },
        Some(block) => Err(eyre!("unexpected block structure {:?}", block)),
        None => Err(eyre!("block {} not found", block_number)),
    }
}
