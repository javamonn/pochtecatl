use alloy::rpc::types::eth::Log;

pub trait ParseableTrade: Clone + Copy  {
    fn parse_from_log(log: &Log, logs: &Vec<Log>, relative_log_idx: usize) -> Option<Self>
    where
        Self: Sized;
}
