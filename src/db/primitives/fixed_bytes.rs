use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

#[repr(transparent)]
#[derive(Debug)]
pub struct FixedBytes<const N: usize>(pub alloy::primitives::FixedBytes<N>);
impl<const N: usize> From<FixedBytes<N>> for alloy::primitives::FixedBytes<N> {
    fn from(value: FixedBytes<N>) -> Self {
        value.0
    }
}

impl<const N: usize> From<alloy::primitives::FixedBytes<N>> for FixedBytes<N> {
    fn from(value: alloy::primitives::FixedBytes<N>) -> Self {
        FixedBytes(value)
    }
}

impl<const N: usize> ToSql for FixedBytes<N> {
    #[inline]
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.0.as_slice()))
    }
}

impl<const N: usize> FromSql for FixedBytes<N> {
    #[inline]
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value {
            ValueRef::Blob(v) => Ok(FixedBytes(alloy::primitives::FixedBytes::from_slice(v))),
            _ => Err(FromSqlError::InvalidType),
        }
    }
}
