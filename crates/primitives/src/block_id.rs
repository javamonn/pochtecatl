use eyre::{eyre, Report};
use std::{fmt::Display, str::FromStr};

#[derive(Copy, Clone)]
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

impl Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockId::Latest => write!(f, "latest"),
            BlockId::BlockNumber(block_number) => write!(f, "{}", block_number),
        }
    }
}
