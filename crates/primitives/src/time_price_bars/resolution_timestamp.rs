use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum Resolution {
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
    #[serde(rename = "4h")]
    FourHours,
}

impl Resolution {
    pub fn offset(&self) -> u64 {
        match self {
            Resolution::FiveMinutes => 300,
            Resolution::OneHour => 3600,
            Resolution::FourHours => 14400,
        }
    }
}

#[derive(PartialOrd, Ord, Eq, PartialEq, Clone, Copy, Debug)]
pub struct ResolutionTimestamp(pub u64);

impl ResolutionTimestamp {
    pub fn from_timestamp(timestamp: u64, resolution: &Resolution) -> Self {
        Self(timestamp - (timestamp % resolution.offset()))
    }

    pub fn decrement(&self, resolution: &Resolution, amount: u64) -> Self {
        Self(self.0 - (resolution.offset() * amount))
    }

    pub fn previous(&self, resolution: &Resolution) -> Self {
        Self(self.0 - resolution.offset())
    }

    pub fn next(&self, resolution: &Resolution) -> Self {
        Self(self.0 + resolution.offset())
    }

    pub fn zero() -> Self {
        Self(0)
    }
}
