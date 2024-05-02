use alloy::rpc::types::eth::Log;
use std::fmt::Debug;

pub trait ParseableTrade: Clone + Copy + Send + Sync + Debug + 'static {
    fn parse_from_log(log: &Log, logs: &Vec<Log>, relative_log_idx: usize) -> Option<Self>
    where
        Self: Sized;
}
