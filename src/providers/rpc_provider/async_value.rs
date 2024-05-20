use eyre::{Context, Result};
use fnv::FnvHashMap;
use tokio::task::JoinSet;

pub struct AsyncValue<T: Clone> {
    value: Option<T>,
    tx: tokio::sync::broadcast::Sender<T>,
}

pub enum AsyncReceiverOrValue<T: Clone> {
    Receiver(tokio::sync::broadcast::Receiver<T>),
    Value(T),
}

impl<T: Clone + Send + 'static> AsyncReceiverOrValue<T> {
    pub async fn resolve_map<K>(
        values: FnvHashMap<K, AsyncReceiverOrValue<Option<T>>>,
    ) -> Result<FnvHashMap<K, T>>
    where
        K: std::hash::Hash + Eq + Clone + Send + 'static,
    {
        let mut output = FnvHashMap::with_capacity_and_hasher(values.len(), Default::default());
        let mut join_set = JoinSet::new();

        for (pair_address, r) in values.into_iter() {
            match r {
                AsyncReceiverOrValue::Value(Some(value)) => {
                    output.insert(pair_address, value);
                }
                AsyncReceiverOrValue::Value(None) => { /* noop: ignore none results in output */ }
                AsyncReceiverOrValue::Receiver(mut receiver) => {
                    let pair_address = pair_address.clone();
                    join_set.spawn(async move {
                        let value = receiver.recv().await;
                        (pair_address, value)
                    });
                }
            }
        }

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((pair_address, Ok(Some(value)))) => {
                    output.insert(pair_address, value);
                }
                Ok((_, Ok(None))) => { /* noop: ignore none results in output */ }
                Ok((_, Err(e))) => {
                    return Err(e).context("Failed to resolve pair metadata async value");
                }
                Err(e) => {
                    return Err(e).context("Failed to resolve pair metadata async value");
                }
            }
        }

        Ok(output)
    }
}

impl<T: Clone> AsyncValue<T> {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(1);
        Self { value: None, tx }
    }

    pub fn get_receiver_or_value(&self) -> AsyncReceiverOrValue<T> {
        if let Some(value) = &self.value {
            AsyncReceiverOrValue::Value(value.clone())
        } else {
            AsyncReceiverOrValue::Receiver(self.tx.subscribe())
        }
    }

    pub fn set(&mut self, value: T) {
        self.value = Some(value.clone());

        // we don't care if receiver has been dropped
        let _ = self.tx.send(value);
    }
}
