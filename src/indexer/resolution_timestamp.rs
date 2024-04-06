#[derive(Debug, Clone, Copy)]
pub enum Resolution {
    FiveMinutes,
}

impl Resolution {
    pub fn offset(&self) -> u64 {
        match self {
            Resolution::FiveMinutes => 300,
        }
    }
}

#[derive(PartialOrd, Ord, Eq, PartialEq, Clone, Copy, Debug)]
pub struct ResolutionTimestamp(u64);

impl ResolutionTimestamp {
    pub fn from_timestamp(timestamp: u64, resolution: &Resolution) -> Self {
        Self(timestamp - (timestamp % resolution.offset()))
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
