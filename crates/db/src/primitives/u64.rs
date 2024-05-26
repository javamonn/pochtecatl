use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

#[repr(transparent)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct U64(pub u64);
impl From<U64> for u64 {
    fn from(u: U64) -> Self {
        u.0
    }
}
impl From<u64> for U64 {
    fn from(u: u64) -> Self {
        Self(u)
    }
}
impl From<alloy::primitives::U64> for U64 {
    fn from(u: alloy::primitives::U64) -> Self {
        U64(u.to::<u64>())
    }
}

// Note that this will wraparound in the DB represenation if the MSB is set, though no
// loss of precision will occur when converting back to U64. This is a limitation of
// SQLite's lack of unsigned integer types.
//
// Querying in DB for values above 2^32-1 should be cooerced to their signed equivalent
// to match the DB representation.
impl ToSql for U64 {
    #[inline]
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        let value = i64::from_le_bytes(self.0.to_le_bytes());
        Ok(ToSqlOutput::from(value))
    }
}

impl FromSql for U64 {
    #[inline]
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value {
            ValueRef::Integer(v) => Ok(U64(u64::from_le_bytes(v.to_le_bytes()))),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}
