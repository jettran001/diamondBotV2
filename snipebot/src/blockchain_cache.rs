use lru::LruCache;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};
use crate::abi_utils;

pub struct CacheEntry<T> {
    data: T,
    expires_at: Instant,
}

pub struct BlockchainCache<K, V> {
    cache: Mutex<LruCache<K, CacheEntry<V>>>,
    ttl: Duration,
}

impl<K: Eq + std::hash::Hash + Clone, V: Clone> BlockchainCache<K, V> {
    pub fn new(capacity: usize, ttl_seconds: u64) -> Self {
        Self {
            cache: Mutex::new(LruCache::new(NonZeroUsize::new(capacity).unwrap())),
            ttl: Duration::from_secs(ttl_seconds),
        }
    }
    
    pub async fn get(&self, key: &K) -> Option<V> {
        let mut cache = self.cache.lock().await;
        if let Some(entry) = cache.get(key) {
            if entry.expires_at > Instant::now() {
                return Some(entry.data.clone());
            } else {
                // Expired
                cache.pop(key);
            }
        }
        None
    }
    
    pub async fn insert(&self, key: K, value: V) {
        let mut cache = self.cache.lock().await;
        let entry = CacheEntry {
            data: value,
            expires_at: Instant::now() + self.ttl,
        };
        cache.put(key, entry);
    }
}
