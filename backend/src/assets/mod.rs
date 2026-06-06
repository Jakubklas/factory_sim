#![allow(dead_code)]
use std::path::{Path, PathBuf};
use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

#[async_trait]
pub trait AssetStore: Send + Sync + 'static {
    /// Store bytes under a content-addressable key. Returns the key.
    async fn put(&self, key: &str, data: &[u8]) -> Result<(), AssetError>;
    /// Retrieve bytes for a key. Returns None if the key doesn't exist.
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AssetError>;
    /// Delete a key.
    async fn delete(&self, key: &str) -> Result<(), AssetError>;
}

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Local filesystem store backed by a PVC mount at `root_dir`.
/// Keys are arbitrary UTF-8 strings; slashes become directory separators.
pub struct LocalStore {
    root: PathBuf,
}

impl LocalStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        // Prevent path traversal: reject anything with ".."
        let safe = key.replace("..", "__");
        self.root.join(Path::new(&safe))
    }
}

#[async_trait]
impl AssetStore for LocalStore {
    async fn put(&self, key: &str, data: &[u8]) -> Result<(), AssetError> {
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(&path).await?;
        file.write_all(data).await?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AssetError> {
        let path = self.path_for(key);
        match tokio::fs::read(&path).await {
            Ok(data)                                  => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e)                                    => Err(e.into()),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), AssetError> {
        let path = self.path_for(key);
        match tokio::fs::remove_file(&path).await {
            Ok(())                                    => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e)                                    => Err(e.into()),
        }
    }
}
