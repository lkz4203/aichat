use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use tokio::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry<T> {
    pub data: T,
    pub created_at: Instant,
    pub accessed_at: Instant,
    pub access_count: u64,
}

impl<T> CacheEntry<T> {
    pub fn new(data: T) -> Self {
        let now = Instant::now();
        Self {
            data,
            created_at: now,
            accessed_at: now,
            access_count: 1,
        }
    }

    pub fn access(&mut self) {
        self.accessed_at = Instant::now();
        self.access_count += 1;
    }

    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.accessed_at.elapsed() > ttl
    }
}

#[derive(Debug)]
pub struct ResponseCache {
    embeddings_cache: Arc<RwLock<LruCache<String, CacheEntry<Vec<f32>>>>>,
    model_responses: Arc<RwLock<LruCache<String, CacheEntry<String>>>>,
    rag_results: Arc<RwLock<LruCache<String, CacheEntry<Vec<Document>>>>>,
    config_cache: Arc<RwLock<LruCache<String, CacheEntry<String>>>>,
    embeddings_ttl: Duration,
    responses_ttl: Duration,
    rag_ttl: Duration,
    config_ttl: Duration,
}

impl Default for ResponseCache {
    fn default() -> Self {
        Self {
            embeddings_cache: Arc::new(RwLock::new(LruCache::new(1000))),
            model_responses: Arc::new(RwLock::new(LruCache::new(500))),
            rag_results: Arc::new(RwLock::new(LruCache::new(200))),
            config_cache: Arc::new(RwLock::new(LruCache::new(100))),
            embeddings_ttl: Duration::from_secs(3600), // 1 hour
            responses_ttl: Duration::from_secs(1800),   // 30 minutes
            rag_ttl: Duration::from_secs(7200),         // 2 hours
            config_ttl: Duration::from_secs(300),       // 5 minutes
        }
    }
}

impl ResponseCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(
        embeddings_cap: usize,
        responses_cap: usize,
        rag_cap: usize,
        config_cap: usize,
    ) -> Self {
        Self {
            embeddings_cache: Arc::new(RwLock::new(LruCache::new(embeddings_cap))),
            model_responses: Arc::new(RwLock::new(LruCache::new(responses_cap))),
            rag_results: Arc::new(RwLock::new(LruCache::new(rag_cap))),
            config_cache: Arc::new(RwLock::new(LruCache::new(config_cap))),
            ..Default::default()
        }
    }

    pub fn with_ttl(
        embeddings_ttl: Duration,
        responses_ttl: Duration,
        rag_ttl: Duration,
        config_ttl: Duration,
    ) -> Self {
        Self {
            embeddings_ttl,
            responses_ttl,
            rag_ttl,
            config_ttl,
            ..Default::default()
        }
    }

    // Embeddings cache methods
    pub fn get_embedding(&self, key: &str) -> Option<Vec<f32>> {
        let mut cache = self.embeddings_cache.write();
        if let Some(entry) = cache.get_mut(key) {
            if entry.is_expired(self.embeddings_ttl) {
                cache.pop(key);
                None
            } else {
                entry.access();
                Some(entry.data.clone())
            }
        } else {
            None
        }
    }

    pub fn set_embedding(&self, key: String, embedding: Vec<f32>) {
        let mut cache = self.embeddings_cache.write();
        cache.put(key, CacheEntry::new(embedding));
    }

    // Model responses cache methods
    pub fn get_response(&self, key: &str) -> Option<String> {
        let mut cache = self.model_responses.write();
        if let Some(entry) = cache.get_mut(key) {
            if entry.is_expired(self.responses_ttl) {
                cache.pop(key);
                None
            } else {
                entry.access();
                Some(entry.data.clone())
            }
        } else {
            None
        }
    }

    pub fn set_response(&self, key: String, response: String) {
        let mut cache = self.model_responses.write();
        cache.put(key, CacheEntry::new(response));
    }

    // RAG results cache methods
    pub fn get_rag_results(&self, key: &str) -> Option<Vec<Document>> {
        let mut cache = self.rag_results.write();
        if let Some(entry) = cache.get_mut(key) {
            if entry.is_expired(self.rag_ttl) {
                cache.pop(key);
                None
            } else {
                entry.access();
                Some(entry.data.clone())
            }
        } else {
            None
        }
    }

    pub fn set_rag_results(&self, key: String, results: Vec<Document>) {
        let mut cache = self.rag_results.write();
        cache.put(key, CacheEntry::new(results));
    }

    // Config cache methods
    pub fn get_config(&self, key: &str) -> Option<String> {
        let mut cache = self.config_cache.write();
        if let Some(entry) = cache.get_mut(key) {
            if entry.is_expired(self.config_ttl) {
                cache.pop(key);
                None
            } else {
                entry.access();
                Some(entry.data.clone())
            }
        } else {
            None
        }
    }

    pub fn set_config(&self, key: String, config: String) {
        let mut cache = self.config_cache.write();
        cache.put(key, CacheEntry::new(config));
    }

    // Cache management
    pub fn clear_all(&self) {
        self.embeddings_cache.write().clear();
        self.model_responses.write().clear();
        self.rag_results.write().clear();
        self.config_cache.write().clear();
    }

    pub fn clear_expired(&self) {
        self.clear_expired_embeddings();
        self.clear_expired_responses();
        self.clear_expired_rag();
        self.clear_expired_config();
    }

    fn clear_expired_embeddings(&self) {
        let mut cache = self.embeddings_cache.write();
        cache.retain(|_, entry| !entry.is_expired(self.embeddings_ttl));
    }

    fn clear_expired_responses(&self) {
        let mut cache = self.model_responses.write();
        cache.retain(|_, entry| !entry.is_expired(self.responses_ttl));
    }

    fn clear_expired_rag(&self) {
        let mut cache = self.rag_results.write();
        cache.retain(|_, entry| !entry.is_expired(self.rag_ttl));
    }

    fn clear_expired_config(&self) {
        let mut cache = self.config_cache.write();
        cache.retain(|_, entry| !entry.is_expired(self.config_ttl));
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            embeddings_count: self.embeddings_cache.read().len(),
            responses_count: self.model_responses.read().len(),
            rag_count: self.rag_results.read().len(),
            config_count: self.config_cache.read().len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub embeddings_count: usize,
    pub responses_count: usize,
    pub rag_count: usize,
    pub config_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub content: String,
    pub metadata: HashMap<String, String>,
    pub embedding: Option<Vec<f32>>,
}

impl Document {
    pub fn new(content: String) -> Self {
        Self {
            content,
            metadata: HashMap::new(),
            embedding: None,
        }
    }

    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn with_embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }
}