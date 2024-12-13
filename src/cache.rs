use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use bytes::Bytes;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::Mutex;
use std::num::NonZeroUsize;

use crate::constants::{CACHE_DIR, MAX_CACHE_SIZE, MAX_FILE_SIZE};

#[derive(Clone, Serialize, Deserialize)]
pub struct CacheMeta {
    pub content_type: String,
    pub is_complete: bool,
    pub total_size: Option<u64>,
}

#[derive(Clone)]
pub struct CacheEntry {
    pub content: Bytes,
    pub meta: CacheMeta,
}

pub struct ProxyCache {
    memory_cache: Arc<Mutex<LruCache<String, CacheEntry>>>,
    cache_dir: PathBuf,
}

impl ProxyCache {
    pub async fn new() -> Result<Self> {
        let cache_dir = PathBuf::from(CACHE_DIR);
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir).await?;
        }
        Ok(ProxyCache {
            memory_cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(MAX_CACHE_SIZE).unwrap()
            ))),
            cache_dir,
        })
    }

    pub async fn get(&self, key: &str) -> Option<CacheEntry> {
        // Try memory cache first
        if let Some(entry) = self.memory_cache.lock().await.get(key).cloned() {
            return Some(entry);
        }

        // Try disk cache
        let file_path = self.cache_dir.join(key);
        if file_path.exists() {
            if let Ok(meta_str) = fs::read_to_string(file_path.with_extension("meta")).await {
                if let Ok(meta) = serde_json::from_str::<CacheMeta>(&meta_str) {
                    if let Ok(content) = fs::read(&file_path).await {
                        let entry = CacheEntry {
                            content: Bytes::from(content),
                            meta,
                        };
                        // 加载到内存缓存
                        if entry.content.len() <= MAX_FILE_SIZE {
                            self.memory_cache.lock().await.put(key.to_string(), entry.clone());
                        }
                        return Some(entry);
                    }
                }
            }
        }
        None
    }

    pub async fn set(&self, key: String, entry: CacheEntry) -> Result<()> {
        // Update memory cache
        if entry.content.len() <= MAX_FILE_SIZE {
            self.memory_cache.lock().await.put(key.clone(), entry.clone());
        }

        // Update disk cache
        let file_path = self.cache_dir.join(key);
        fs::write(&file_path, &entry.content).await?;
        fs::write(
            file_path.with_extension("meta"),
            serde_json::to_string(&entry.meta)?,
        ).await?;
        Ok(())
    }
}
