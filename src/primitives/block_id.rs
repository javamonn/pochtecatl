use eyre::{eyre, Report};
use log::kv::ToValue;
use std::str::FromStr;

pub enum BlockId {
    Latest,
    BlockNumber(alloy::primitives::BlockNumber),
}

#[derive(Debug, PartialEq)]
pub struct ParseBlockIdError(&'static str);

impl FromStr for BlockId {
    type Err = Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse::<u64>() {
            Ok(block_number) => Ok(BlockId::BlockNumber(block_number)),
            Err(_) if s == "latest" => Ok(BlockId::Latest),
            _ => Err(eyre!("Failed to parse block id: {}", s)),
        }
    }
}

impl ToValue for BlockId {
    fn to_value(&self) -> log::kv::Value {
        match self {
            BlockId::Latest => log::kv::Value::from("latest"),
            BlockId::BlockNumber(number) => log::kv::Value::from(number),
        }
    }
}
