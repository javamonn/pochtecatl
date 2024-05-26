use std::time::{SystemTime, UNIX_EPOCH};

pub struct TTLCache<V> {
    value: V,
    ttl: Option<std::time::Duration>,
}

impl<V> TTLCache<V> {
    pub fn new(value: V, ttl: Option<std::time::Duration>) -> Self {
        Self { value, ttl }
    }

    pub fn value(&self) -> &V {
        &self.value
    }

    pub fn is_expired(&self) -> bool {
        match self.ttl {
            None => false,
            Some(ttl) => {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("Time went backwards")
                    < ttl
            }
        }
    }
}
