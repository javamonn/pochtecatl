// LRU cache wrapper that supports deduping async resolutions

use std::{future::Future, hash::Hash, num::NonZeroUsize};

use lru::LruCache;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast::{Receiver, Sender};

enum AsyncValue<V> {
    Pending(Sender<Result<V, String>>),
    Resolved(Result<V, String>),
}

enum ValueOrReceiver<V> {
    Value(Result<V, String>),
    Receiver(Receiver<Result<V, String>>),
}

type Resolver<K, C, V> =
    Box<dyn Fn(K, C) -> Pin<Box<dyn Future<Output = eyre::Result<V>> + Send>> + Send + Sync>;

pub struct AsyncLruCache<K, V, C> {
    inner: Arc<Mutex<LruCache<K, AsyncValue<V>>>>,
    resolver: Arc<Resolver<K, C, V>>,
}

impl<K, V, C> AsyncLruCache<K, V, C>
where
    K: Hash + PartialEq + Eq + Clone + Send + 'static,
    V: Clone + Send + 'static,
    C: Send + 'static,
{
    pub fn new(capacity: NonZeroUsize, resolver: Resolver<K, C, V>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LruCache::new(capacity))),
            resolver: Arc::new(resolver),
        }
    }

    pub async fn get_or_resolve(&self, key: &K, context: C) -> Result<V, String>
where {
        let (sender, state_value) = {
            let mut inner = self.inner.lock().unwrap();
            match inner.get(key) {
                Some(AsyncValue::Resolved(value)) => (None, ValueOrReceiver::Value(value.clone())),
                Some(AsyncValue::Pending(sender)) => {
                    (None, ValueOrReceiver::Receiver(sender.subscribe()))
                }
                None => {
                    let (sender, receiver) = tokio::sync::broadcast::channel(1);
                    inner.put(key.clone(), AsyncValue::Pending(sender.clone()));

                    (Some(sender), ValueOrReceiver::Receiver(receiver))
                }
            }
        };

        if let Some(sender) = sender {
            let inner = Arc::clone(&self.inner);
            let resolver = Arc::clone(&self.resolver);
            let key = key.clone();

            tokio::spawn(async move {
                let value = (resolver)(key.clone(), context)
                    .await
                    .map_err(|e| format!("Failed to resolve value: {:?}", e));

                {
                    let mut inner = inner.lock().unwrap();
                    inner.put(key, AsyncValue::Resolved(value.clone()));
                }

                let _ = sender.send(value);
            });
        }

        match state_value {
            ValueOrReceiver::Value(value) => value,
            ValueOrReceiver::Receiver(mut receiver) => receiver
                .recv()
                .await
                .map_err(|err| err.to_string())
                .and_then(|v| v),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::AsyncLruCache;

    use eyre::Result;
    use std::{
        num::NonZeroUsize,
        sync::{Arc, Mutex},
    };

    async fn resolver(key: u32, context: Arc<Mutex<u32>>) -> Result<u32> {
        let mut context = context.lock().unwrap();
        *context += key;
        Ok(*context)
    }

    #[tokio::test]
    pub async fn test_get_or_resolve() -> Result<()> {
        let resolver_context = Arc::new(Mutex::new(0_u32));
        let cache = AsyncLruCache::new(
            NonZeroUsize::new(10).unwrap(),
            Box::new(|key, context| Box::pin(resolver(key, context))),
        );

        // Test concurrent fetches
        let (value_1, value_2) = tokio::join!(
            cache.get_or_resolve(&1, resolver_context.clone()),
            cache.get_or_resolve(&1, resolver_context.clone())
        );
        assert_eq!(value_1.expect("Expected value 1"), 1);
        assert_eq!(value_2.expect("Expected value 2"), 1);
        assert_eq!(*resolver_context.lock().unwrap(), 1);

        // Test cache after value is resolved
        let value_3 = cache
            .get_or_resolve(&1, resolver_context.clone())
            .await
            .expect("Expected value 3");
        assert_eq!(value_3, 1);
        assert_eq!(*resolver_context.lock().unwrap(), 1);

        Ok(())
    }
}
