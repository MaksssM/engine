use crate::asset_database::{AssetId, AssetType};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt, fs,
    hash::Hash,
    io,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Configuration for asset storage
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetStorageConfig {
    /// Root directory for the project
    pub project_root: PathBuf,
    /// Directory for imported asset data
    pub imported_dir: PathBuf,
    /// Directory for cache
    pub cache_dir: PathBuf,
    /// Maximum cache size in bytes (0 = unlimited)
    pub max_cache_size: u64,
    /// Asset importer version
    pub importer_version: u32,
}

impl Default for AssetStorageConfig {
    fn default() -> Self {
        Self {
            project_root: PathBuf::from("."),
            imported_dir: PathBuf::from("Assets/Imported"),
            cache_dir: PathBuf::from("Assets/Cache"),
            max_cache_size: 0,
            importer_version: 1,
        }
    }
}

/// Asset file info for storage
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetFileInfo {
    pub asset_id: AssetId,
    pub asset_type: AssetType,
    pub file_path: PathBuf,
    pub hash: String,
    pub size: u64,
    pub created_at: SystemTime,
    pub modified_at: SystemTime,
}

/// Index of all stored assets
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct AssetIndex {
    pub files: HashMap<AssetId, AssetFileInfo>,
    pub by_type: HashMap<AssetType, Vec<AssetId>>,
    pub by_hash: HashMap<String, AssetId>,
}

/// Compute hash of a file
pub fn compute_file_hash(path: &Path) -> io::Result<String> {
    use std::hash::{DefaultHasher, Hasher};
    let mut hasher = DefaultHasher::new();
    let content = fs::read(path)?;
    content.hash(&mut hasher);
    Ok(format!("{:x}", hasher.finish()))
}

/// Compute hash of bytes
pub fn compute_bytes_hash(bytes: &[u8]) -> String {
    use std::hash::{DefaultHasher, Hasher};
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Asset storage manager
pub struct AssetStorage {
    config: AssetStorageConfig,
    index: AssetIndex,
}

impl AssetStorage {
    pub fn new(config: AssetStorageConfig) -> io::Result<Self> {
        // Create directories
        fs::create_dir_all(&config.imported_dir)?;
        fs::create_dir_all(&config.cache_dir)?;

        // Load index
        let index_path = config.cache_dir.join("asset_index.ron");
        let index = if index_path.exists() {
            let content = fs::read_to_string(&index_path)?;
            ron::from_str(&content).unwrap_or_default()
        } else {
            AssetIndex::default()
        };

        Ok(Self { config, index })
    }

    /// Save the index to disk
    pub fn save_index(&self) -> io::Result<()> {
        let index_path = self.config.cache_dir.join("asset_index.ron");
        let content = ron::to_string(&self.index)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(&index_path, content)
    }

    /// Get the storage path for an asset type
    pub fn get_type_dir(&self, asset_type: AssetType) -> PathBuf {
        let type_name = match asset_type {
            AssetType::Mesh => "Meshes",
            AssetType::Material => "Materials",
            AssetType::Texture => "Textures",
            AssetType::Model => "Models",
            AssetType::Shader => "Shaders",
            AssetType::Scene => "Scenes",
            AssetType::Audio => "Audio",
            AssetType::Script => "Scripts",
            AssetType::Other => "Other",
        };
        self.config.imported_dir.join(type_name)
    }

    /// Get the full path for an asset file
    pub fn get_asset_path(&self, asset_id: AssetId, asset_type: AssetType) -> PathBuf {
        let type_dir = self.get_type_dir(asset_type);
        type_dir.join(format!("{}.asset", asset_id))
    }

    /// Store asset data to disk
    pub fn store_asset(
        &mut self,
        asset_id: AssetId,
        asset_type: AssetType,
        data: &[u8],
    ) -> io::Result<()> {
        let type_dir = self.get_type_dir(asset_type);
        fs::create_dir_all(&type_dir)?;

        let asset_path = type_dir.join(format!("{}.asset", asset_id));
        let hash = compute_bytes_hash(data);
        let size = data.len() as u64;
        let now = SystemTime::now();

        // Write asset data
        fs::write(&asset_path, data)?;

        // Update index
        let file_info = AssetFileInfo {
            asset_id,
            asset_type,
            file_path: asset_path.clone(),
            hash: hash.clone(),
            size,
            created_at: now,
            modified_at: now,
        };

        self.index.files.insert(asset_id, file_info.clone());
        self.index
            .by_type
            .entry(asset_type)
            .or_default()
            .push(asset_id);
        self.index.by_hash.insert(hash, asset_id);

        // Save index
        self.save_index()?;

        Ok(())
    }

    /// Load asset data from disk
    pub fn load_asset(&self, asset_id: AssetId, asset_type: AssetType) -> io::Result<Vec<u8>> {
        let asset_path = self.get_asset_path(asset_id, asset_type);
        fs::read(&asset_path)
    }

    /// Check if asset exists in storage
    pub fn has_asset(&self, asset_id: AssetId) -> bool {
        self.index.files.contains_key(&asset_id)
    }

    /// Get asset file info
    pub fn get_file_info(&self, asset_id: AssetId) -> Option<&AssetFileInfo> {
        self.index.files.get(&asset_id)
    }

    /// Remove asset from storage
    pub fn remove_asset(&mut self, asset_id: AssetId) -> io::Result<bool> {
        if let Some(file_info) = self.index.files.remove(&asset_id) {
            // Remove from type index
            if let Some(vec) = self.index.by_type.get_mut(&file_info.asset_type) {
                vec.retain(|id| *id != asset_id);
            }
            // Remove from hash index
            self.index.by_hash.remove(&file_info.hash);

            // Delete file
            if file_info.file_path.exists() {
                fs::remove_file(&file_info.file_path)?;
            }

            // Save index
            self.save_index()?;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get all assets of a type
    pub fn get_assets_by_type(&self, asset_type: AssetType) -> Vec<AssetId> {
        self.index
            .by_type
            .get(&asset_type)
            .cloned()
            .unwrap_or_default()
    }

    /// Find asset by hash (for deduplication)
    pub fn find_by_hash(&self, hash: &str) -> Option<AssetId> {
        self.index.by_hash.get(hash).copied()
    }

    /// Get total storage size
    pub fn total_size(&self) -> u64 {
        self.index.files.values().map(|f| f.size).sum()
    }

    /// Get storage config
    pub fn config(&self) -> &AssetStorageConfig {
        &self.config
    }

    /// Verify asset integrity by checking hash
    pub fn verify_asset(&self, asset_id: AssetId) -> io::Result<bool> {
        if let Some(file_info) = self.index.files.get(&asset_id) {
            let data = fs::read(&file_info.file_path)?;
            let hash = compute_bytes_hash(&data);
            Ok(hash == file_info.hash)
        } else {
            Ok(false)
        }
    }

    /// Clean up orphaned files
    pub fn cleanup_orphans(&mut self) -> io::Result<usize> {
        let mut removed = 0;
        let mut to_remove = Vec::new();

        for (id, file_info) in &self.index.files {
            if !file_info.file_path.exists() {
                to_remove.push(*id);
            }
        }

        for id in to_remove {
            self.remove_asset(id)?;
            removed += 1;
        }

        Ok(removed)
    }
}

/// Cache entry for loaded asset data
#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub asset_id: AssetId,
    pub data: Vec<u8>,
    pub last_accessed: SystemTime,
    pub access_count: u64,
}

/// In-memory cache for frequently accessed assets
pub struct AssetCache {
    entries: HashMap<AssetId, CacheEntry>,
    max_size: u64,
    current_size: u64,
}

impl AssetCache {
    pub fn new(max_size: u64) -> Self {
        Self {
            entries: HashMap::new(),
            max_size,
            current_size: 0,
        }
    }

    pub fn get(&mut self, asset_id: AssetId) -> Option<&[u8]> {
        if let Some(entry) = self.entries.get_mut(&asset_id) {
            entry.last_accessed = SystemTime::now();
            entry.access_count += 1;
            Some(&entry.data)
        } else {
            None
        }
    }

    pub fn peek(&self, asset_id: AssetId) -> Option<&[u8]> {
        self.entries.get(&asset_id).map(|e| e.data.as_slice())
    }

    pub fn insert(&mut self, asset_id: AssetId, data: Vec<u8>) {
        let size = data.len() as u64;

        // Evict if needed
        while self.current_size + size > self.max_size
            && self.max_size > 0
            && !self.entries.is_empty()
        {
            self.evict_lru();
        }

        if self.max_size > 0 && size > self.max_size {
            // Single entry larger than max cache size, don't cache
            return;
        }

        let entry = CacheEntry {
            asset_id,
            data,
            last_accessed: SystemTime::now(),
            access_count: 1,
        };
        self.current_size += size;
        self.entries.insert(asset_id, entry);
    }

    pub fn remove(&mut self, asset_id: AssetId) -> Option<Vec<u8>> {
        if let Some(entry) = self.entries.remove(&asset_id) {
            self.current_size -= entry.data.len() as u64;
            Some(entry.data)
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_size = 0;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn current_size(&self) -> u64 {
        self.current_size
    }

    fn evict_lru(&mut self) {
        if let Some((lru_id, _)) = self.entries.iter().min_by_key(|(_, e)| e.last_accessed) {
            let lru_id = *lru_id;
            self.remove(lru_id);
        }
    }

    pub fn contains(&self, asset_id: AssetId) -> bool {
        self.entries.contains_key(&asset_id)
    }
}

impl fmt::Debug for AssetCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AssetCache")
            .field("entries", &self.entries.len())
            .field("current_size", &self.current_size)
            .field("max_size", &self.max_size)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn asset_storage_basic() {
        let temp = TempDir::new().unwrap();
        let config = AssetStorageConfig {
            project_root: temp.path().to_path_buf(),
            imported_dir: temp.path().join("Assets/Imported"),
            cache_dir: temp.path().join("Assets/Cache"),
            max_cache_size: 0,
            importer_version: 1,
        };

        let mut storage = AssetStorage::new(config).unwrap();
        let id = AssetId::new();
        let data = b"test asset data";

        storage.store_asset(id, AssetType::Mesh, data).unwrap();
        assert!(storage.has_asset(id));

        let loaded = storage.load_asset(id, AssetType::Mesh).unwrap();
        assert_eq!(loaded, data);

        storage.remove_asset(id).unwrap();
        assert!(!storage.has_asset(id));
    }

    #[test]
    fn asset_storage_deduplication() {
        let temp = TempDir::new().unwrap();
        let config = AssetStorageConfig {
            project_root: temp.path().to_path_buf(),
            imported_dir: temp.path().join("Assets/Imported"),
            cache_dir: temp.path().join("Assets/Cache"),
            max_cache_size: 0,
            importer_version: 1,
        };

        let mut storage = AssetStorage::new(config).unwrap();
        let id1 = AssetId::new();
        let id2 = AssetId::new();
        let data = b"same data";

        storage.store_asset(id1, AssetType::Mesh, data).unwrap();
        let hash = compute_bytes_hash(data);
        assert_eq!(storage.find_by_hash(&hash), Some(id1));

        // Store same data with different ID - should find existing
        let existing = storage.find_by_hash(&hash);
        assert_eq!(existing, Some(id1));
    }

    #[test]
    fn asset_cache_lru() {
        let mut cache = AssetCache::new(100); // 100 bytes max
        let id1 = AssetId::new();
        let id2 = AssetId::new();
        let id3 = AssetId::new();

        cache.insert(id1, vec![1u8; 40]);
        cache.insert(id2, vec![2u8; 40]);
        cache.insert(id3, vec![3u8; 40]); // Should evict id1

        assert!(!cache.contains(id1)); // Evicted
        assert!(cache.contains(id2));
        assert!(cache.contains(id3));
    }
}
