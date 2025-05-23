use crate::primitives::{AppError, AppJson, AppState};

use pochtecatl_db::{BacktestClosedTradeModel, BlockModel};
use pochtecatl_primitives::{
    u32f96_from_u256_frac, Block, IndicatorsConfig, Resolution, ResolutionTimestamp, TimePriceBar,
    TimePriceBars, TradeMetadata, TradeRequestOp,
};

use alloy::primitives::{uint, Address, TxHash, U256};
use axum::extract::{Path, Query, State};
use eyre::Result;
use fixed::traits::LossyInto;
use lazy_static::lazy_static;
use rusqlite::Transaction;
use serde::{Deserialize, Serialize};
use tracing::error;

lazy_static! {
    static ref WETH_DECIMALS_FACTOR: U256 = uint!(10_U256).pow(uint!(18_U256));
}

#[derive(Serialize)]
struct PriceTickResponse {
    resolution_ts: u64,

    // ohlc
    open: f64,
    high: f64,
    low: f64,
    close: f64,

    // indicators
    ema: Option<f64>,
    sma: Option<f64>,
}
impl PriceTickResponse {
    pub fn new(
        resolution_ts: u64,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        ema: Option<f64>,
        sma: Option<f64>,
    ) -> Self {
        Self {
            resolution_ts,
            open,
            high,
            low,
            close,
            ema,
            sma,
        }
    }

    pub fn from_time_price_bar(ts: ResolutionTimestamp, time_price_bar: &TimePriceBar) -> Self {
        let ema = time_price_bar.indicators().map(|i| i.ema.0.lossy_into());
        let sma = time_price_bar
            .indicators()
            .and_then(|i| i.bollinger_bands)
            .map(|i| i.0.lossy_into());

        Self::new(
            ts.0,
            (*time_price_bar.open()).lossy_into(),
            (*time_price_bar.high()).lossy_into(),
            (*time_price_bar.low()).lossy_into(),
            (*time_price_bar.close()).lossy_into(),
            ema,
            sma,
        )
    }
}

#[derive(Serialize)]
struct TradeTickResponse {
    closes_tx_hash: Option<TxHash>,
    tx_hash: TxHash,
    resolution_ts: u64,
    ts: u64,
    execution_price: f64,  // The effective token price in ETH for the trade
    execution_amount: f64, // The ETH amount sent for the buy or received for a sell
    gas_fee_amount: f64,   // The ETH amount paid for the base and pri fee
}

impl TradeTickResponse {
    pub fn try_from_trade_metadata(value: TradeMetadata, resolution: &Resolution) -> Result<Self> {
        let execution_price: f64 = value
            .indexed_trade()
            .token_price_after(value.token_address())
            .lossy_into();

        let execution_amount: f64 = u32f96_from_u256_frac(
            value.indexed_trade().weth_volume(value.token_address()),
            *WETH_DECIMALS_FACTOR,
        )
        .lossy_into();
        let gas_fee_amount: f64 =
            u32f96_from_u256_frac(*value.gas_fee(), *WETH_DECIMALS_FACTOR).lossy_into();

        Ok(Self {
            closes_tx_hash: match value.op() {
                TradeRequestOp::Close {
                    open_trade_tx_hash, ..
                } => Some(open_trade_tx_hash.clone()),
                _ => None,
            },
            tx_hash: value.tx_hash().clone(),
            resolution_ts: ResolutionTimestamp::from_timestamp(
                *value.block_timestamp(),
                resolution,
            )
            .0,
            ts: *value.block_timestamp(),
            execution_price,
            execution_amount,
            gas_fee_amount,
        })
    }
}

#[derive(Serialize)]
pub struct Response {
    price_ticks: Vec<PriceTickResponse>,
    trade_ticks: Vec<TradeTickResponse>,
}

fn get_trades(
    tx: &Transaction,
    backtest_id: i64,
    pair_address: Address,
    start_at: u64,
    end_at: u64,
    resolution: &Resolution,
) -> Result<Vec<TradeTickResponse>> {
    BacktestClosedTradeModel::query_by_backtest_pair_timestamp(
        &tx,
        backtest_id,
        pair_address,
        start_at,
        end_at,
    )
    .map(|trades| {
        let initial = Vec::with_capacity(trades.len() * 2);
        trades.into_iter().fold(initial, |mut acc, trade| {
            match TradeMetadata::deserialize(trade.open_trade_metadata)
                .map_err(Into::into)
                .and_then(|trade_metadata| {
                    TradeTickResponse::try_from_trade_metadata(trade_metadata, resolution)
                }) {
                Ok(open_trade) => acc.push(open_trade),
                Err(e) => error!("Failed to deserialize trade metadata: {}", e),
            };

            match TradeMetadata::deserialize(trade.close_trade_metadata)
                .map_err(Into::into)
                .and_then(|trade_metadata| {
                    TradeTickResponse::try_from_trade_metadata(trade_metadata, &resolution)
                }) {
                Ok(close_trade) => acc.push(close_trade),
                Err(e) => error!("Failed to deserialize trade metadata: {}", e),
            };

            acc
        })
    })
}

fn get_price_ticks(
    tx: &Transaction,
    pair_address: Address,
    start_at: u64,
    end_at: u64,
    resolution: Resolution,
) -> eyre::Result<Vec<PriceTickResponse>, AppError> {
    let blocks = BlockModel::query_by_timestamp_range(&tx, start_at, end_at)?;
    let pair_time_price_bars = blocks
        .into_iter()
        .filter_map(|block| {
            let block = Block::from(block);

            block.pair_ticks.get(&pair_address).map(|pair_block_tick| {
                (
                    block.block_number,
                    block.block_timestamp,
                    pair_block_tick.tick().clone(),
                )
            })
        })
        .fold(
            TimePriceBars::new(None, resolution, Some(IndicatorsConfig::All)),
            |mut acc, (block_number, block_timestamp, tick)| {
                let _ = acc
                    .insert_data(
                        block_number,
                        tick,
                        block_timestamp,
                        Some(
                            ResolutionTimestamp::from_timestamp(block_timestamp, &resolution)
                                .previous(&resolution),
                        ),
                    )
                    .inspect_err(|e| error!("Failed to insert data: {}", e));
                acc
            },
        );

    Ok(pair_time_price_bars
        .data()
        .iter()
        .map(|(ts, time_price_bar)| PriceTickResponse::from_time_price_bar(*ts, time_price_bar))
        .collect())
}

#[derive(Deserialize)]
pub struct Params {
    start_at: u64,
    end_at: u64,
    resolution: Resolution,
}

pub async fn handler(
    Path((backtest_id, pair_address)): Path<(i64, Address)>,
    State(app_state): State<AppState>,
    Query(Params {
        start_at,
        end_at,
        resolution,
    }): Query<Params>,
) -> eyre::Result<AppJson<Response>, AppError> {
    let mut db_conn = app_state.db().get()?;
    let tx = db_conn.transaction()?;
    let trade_ticks = get_trades(&tx, backtest_id, pair_address, start_at, end_at, &resolution)?;
    let price_ticks = get_price_ticks(&tx, pair_address, start_at, end_at, resolution)?;
    tx.finish()?;

    Ok(AppJson(Response {
        price_ticks,
        trade_ticks,
    }))
}
