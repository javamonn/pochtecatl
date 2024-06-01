use crate::primitives::{AppError, AppJson, AppState};

use pochtecatl_db::{BacktestClosedTradeModel, BacktestTimePriceBarModel, BlockModel};
use pochtecatl_primitives::{
    u32f96_from_u256_frac, Block, FinalizedTimePriceBar, IndicatorsConfig, Resolution,
    ResolutionTimestamp, TimePriceBar, TimePriceBars, TradeMetadata, TradeRequestOp,
};

use alloy::primitives::{uint, Address, TxHash, U256};
use axum::extract::{Path, Query, State};
use eyre::{Context, Result};
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

    pub fn from_finalized_time_price_bar(ts: u64, time_price_bar: &FinalizedTimePriceBar) -> Self {
        let ema = time_price_bar.indicators().map(|i| i.ema.0.lossy_into());
        let sma = time_price_bar
            .indicators()
            .and_then(|i| i.bollinger_bands)
            .map(|i| i.0.lossy_into());

        Self::new(
            ts,
            time_price_bar.data.open.lossy_into(),
            time_price_bar.data.high.lossy_into(),
            time_price_bar.data.low.lossy_into(),
            time_price_bar.data.close.lossy_into(),
            ema,
            sma,
        )
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
    pnl: f64,
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

fn get_price_ticks_from_time_price_bars(
    tx: &Transaction,
    pair_address: Address,
    start_at: u64,
    end_at: u64,
) -> eyre::Result<Vec<PriceTickResponse>, AppError> {
    let time_price_bars = BacktestTimePriceBarModel::query_by_pair_resolution_ts(
        &tx,
        pair_address,
        start_at,
        end_at,
    )?;

    let mut output = Vec::with_capacity(time_price_bars.len());
    for time_price_bar in time_price_bars.into_iter() {
        let finalized_time_price_bar =
            serde_json::from_value(time_price_bar.data).wrap_err("Failed to deserialize data")?;
        output.push(PriceTickResponse::from_finalized_time_price_bar(
            time_price_bar.resolution_ts.0,
            &finalized_time_price_bar,
        ));
    }

    Ok(output)
}

fn calculate_pnl(trade_ticks: &Vec<TradeTickResponse>) -> f64 {
    trade_ticks.iter().enumerate().fold(0.0, |acc, (idx, trade_tick)| {
        if trade_tick.closes_tx_hash.is_some() {
            acc + trade_tick.execution_amount - trade_tick.gas_fee_amount
        } else if idx != trade_ticks.len() - 1 {
            // Exclude the last trade if its an open trade since it has no corresponing
            // close within the backtest range.
            acc - trade_tick.execution_amount - trade_tick.gas_fee_amount
        } else {
            acc
        }
    })
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
    let trade_ticks = get_trades(
        &tx,
        backtest_id,
        pair_address,
        start_at,
        end_at,
        &resolution,
    )?;
    let price_ticks = get_price_ticks_from_time_price_bars(&tx, pair_address, start_at, end_at)?;
    tx.finish()?;

    let pnl = calculate_pnl(&trade_ticks);

    Ok(AppJson(Response {
        price_ticks,
        trade_ticks,
        pnl,
    }))
}
