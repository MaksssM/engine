use crate::asset_database::{AssetDatabase, AssetId, AssetLoadState, AssetType};
use crate::asset_storage::{AssetCache, AssetStorage};
use bevy_ecs::resource::Resource;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, marker::PhantomData, path::PathBuf, sync::Arc};

#[derive(Debug, Serialize, Deserialize)]
pub struct Handle<T> {
    id: AssetId,
    #[serde(skip)]
    _marker: PhantomData<fn() -> T>,
}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Handle<T> {}

impl<T> std::hash::Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> Copy for Handle<T> {}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Handle<T> {
    pub fn new(id: AssetId) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    pub fn from_id(id: u64) -> Self {
        Self {
            id: AssetId::from_u64(id),
            _marker: PhantomData,
        }
    }

    pub fn id(self) -> u64 {
        self.id.as_u64()
    }

    pub fn asset_id(self) -> AssetId {
        self.id
    }
}

impl<T> Default for Handle<T> {
    fn default() -> Self {
        Self {
            id: AssetId::nil(),
            _marker: PhantomData,
        }
    }
}

/// Events for asset operations
#[derive(Debug, Clone)]
pub enum AssetEvent {
    Loaded(AssetId),
    Failed(AssetId),
    Unloaded(AssetId),
    Reloaded(AssetId),
}

/// Trait for assets that can be loaded from storage
pub trait Asset: Send + Sync + 'static {
    fn asset_type() -> AssetType;
}

/// Asset data that can be serialized/deserialized
pub trait AssetData: Send + Sync + 'static + std::fmt::Debug {
    fn from_bytes(bytes: &[u8]) -> Result<Self, String>
    where
        Self: Sized;
    fn to_bytes(&self) -> Vec<u8>;
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl<T: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static + std::fmt::Debug> AssetData
    for T
{
    fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        ron::de::from_bytes(bytes).map_err(|e| e.to_string())
    }

    fn to_bytes(&self) -> Vec<u8> {
        ron::ser::to_string(self).unwrap().into_bytes()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// In-memory asset manager for loaded resources (GPU meshes, materials, textures, etc.)
#[derive(Debug, Resource)]
pub struct AssetManager<T, K = String> {
    assets: HashMap<Handle<T>, Arc<T>>,
    paths: HashMap<K, Handle<T>>,
    by_asset_id: HashMap<AssetId, Handle<T>>,
    next_id: u64,
}

impl<T, K: Eq + std::hash::Hash> Default for AssetManager<T, K> {
    fn default() -> Self {
        Self {
            assets: HashMap::new(),
            paths: HashMap::new(),
            by_asset_id: HashMap::new(),
            next_id: 1,
        }
    }
}

impl<T, K: Eq + std::hash::Hash> AssetManager<T, K> {
    pub fn insert(&mut self, key: K, asset: T) -> Handle<T> {
        let handle = Handle::from_id(self.next_id);
        self.next_id += 1;
        self.by_asset_id.insert(handle.asset_id(), handle);
        self.assets.insert(handle, Arc::new(asset));
        self.paths.insert(key, handle);
        handle
    }

    pub fn insert_with_id(&mut self, key: K, asset_id: AssetId, asset: T) -> Handle<T> {
        let handle = Handle::new(asset_id);
        self.by_asset_id.insert(asset_id, handle);
        self.assets.insert(handle, Arc::new(asset));
        self.paths.insert(key, handle);
        handle
    }

    pub fn replace(&mut self, key: &K, asset: T) -> Option<Handle<T>> {
        let handle = self.paths.get(key).copied()?;
        self.assets.insert(handle, Arc::new(asset));
        Some(handle)
    }

    pub fn get(&self, handle: Handle<T>) -> Option<Arc<T>> {
        self.assets.get(&handle).cloned()
    }

    pub fn get_ref(&self, handle: Handle<T>) -> Option<&T> {
        self.assets.get(&handle).map(Arc::as_ref)
    }

    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.assets.get_mut(&handle).and_then(Arc::get_mut)
    }

    pub fn get_by_asset_id(&self, asset_id: AssetId) -> Option<Handle<T>> {
        self.by_asset_id.get(&asset_id).copied()
    }

    pub fn get_by_key(&self, key: &K) -> Option<Handle<T>> {
        self.paths.get(key).copied()
    }

    pub fn insert_key(&mut self, key: K, handle: Handle<T>) {
        self.paths.insert(key, handle);
    }

    pub fn remove_key(&mut self, key: &K) -> Option<Handle<T>> {
        self.paths.remove(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.paths.keys()
    }

    pub fn load_cached<F>(&mut self, key: K, loader: F) -> Handle<T>
    where
        F: FnOnce() -> T,
    {
        if let Some(handle) = self.paths.get(&key).copied() {
            return handle;
        }
        self.insert(key, loader())
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    pub fn remove(&mut self, handle: Handle<T>) -> Option<Arc<T>> {
        self.by_asset_id.remove(&handle.asset_id());
        self.paths.retain(|_, v| *v != handle);
        self.assets.remove(&handle)
    }

    pub fn remove_by_asset_id(&mut self, asset_id: AssetId) -> Option<Arc<T>> {
        if let Some(handle) = self.by_asset_id.remove(&asset_id) {
            self.paths.retain(|_, v| *v != handle);
            self.assets.remove(&handle)
        } else {
            None
        }
    }
}

/// Kind of GPU resource uploaded to the graphics device
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GpuResourceKind {
    Mesh,
    Material,
    Texture,
}

/// Lifecycle status of a GPU-side resource
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GpuResourceStatus {
    /// Queued or requested, not yet uploaded
    PendingUpload,
    /// Successfully uploaded and valid on the GPU
    Ready,
    /// Asset revision has changed; GPU copy is outdated
    Stale,
    /// Failed to upload or invalid data
    Failed,
    /// Explicitly freed or unloaded from the GPU
    Unloaded,
}

/// Metadata entry for a single GPU-resident resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuResourceEntry {
    pub asset_id: AssetId,
    pub revision: u64,
    pub kind: GpuResourceKind,
    pub handle_id: u64,
    pub status: GpuResourceStatus,
    pub size_bytes: usize,
    pub last_used: std::time::SystemTime,
}

/// Detailed instrumentation statistics for GPU resources
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuResourceStats {
    pub uploaded_meshes: usize,
    pub uploaded_textures: usize,
    pub uploaded_materials: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub reimport_count: usize,
    pub stale_count: usize,
    pub approximate_memory_bytes: usize,
}

/// Registry tracking all GPU resource allocations, revisions, and lifecycle states
#[derive(Debug, Default, Resource, Clone)]
pub struct GpuResourceRegistry {
    entries: HashMap<AssetId, GpuResourceEntry>,
    cache_hits: usize,
    cache_misses: usize,
    reimport_count: usize,
    uploaded_meshes: usize,
    uploaded_textures: usize,
    uploaded_materials: usize,
}

impl GpuResourceRegistry {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            cache_hits: 0,
            cache_misses: 0,
            reimport_count: 0,
            uploaded_meshes: 0,
            uploaded_textures: 0,
            uploaded_materials: 0,
        }
    }

    /// Check if the GPU resource for `asset_id` exists, is in `Ready` status, and matches `revision`.
    pub fn is_up_to_date(&self, asset_id: AssetId, revision: u64) -> bool {
        if let Some(entry) = self.entries.get(&asset_id) {
            entry.status == GpuResourceStatus::Ready && entry.revision == revision
        } else {
            false
        }
    }

    /// Get the resource entry for an asset
    pub fn get(&self, asset_id: AssetId) -> Option<&GpuResourceEntry> {
        self.entries.get(&asset_id)
    }

    /// Register or update an uploaded GPU resource
    pub fn register_upload(
        &mut self,
        asset_id: AssetId,
        revision: u64,
        kind: GpuResourceKind,
        handle_id: u64,
        size_bytes: usize,
    ) {
        self.entries.insert(
            asset_id,
            GpuResourceEntry {
                asset_id,
                revision,
                kind,
                handle_id,
                status: GpuResourceStatus::Ready,
                size_bytes,
                last_used: std::time::SystemTime::now(),
            },
        );
    }

    /// Mark a resource as pending upload
    pub fn mark_pending(&mut self, asset_id: AssetId, revision: u64, kind: GpuResourceKind) {
        if let Some(entry) = self.entries.get_mut(&asset_id) {
            entry.revision = revision;
            entry.kind = kind;
            entry.status = GpuResourceStatus::PendingUpload;
        } else {
            self.entries.insert(
                asset_id,
                GpuResourceEntry {
                    asset_id,
                    revision,
                    kind,
                    handle_id: 0,
                    status: GpuResourceStatus::PendingUpload,
                    size_bytes: 0,
                    last_used: std::time::SystemTime::now(),
                },
            );
        }
    }

    /// Mark a resource as stale (needs re-upload due to reimport)
    pub fn mark_stale(&mut self, asset_id: AssetId) {
        if let Some(entry) = self.entries.get_mut(&asset_id) {
            entry.status = GpuResourceStatus::Stale;
        }
    }

    /// Mark a resource as failed
    pub fn mark_failed(&mut self, asset_id: AssetId) {
        if let Some(entry) = self.entries.get_mut(&asset_id) {
            entry.status = GpuResourceStatus::Failed;
        }
    }

    /// Mark a resource as unloaded
    pub fn mark_unloaded(&mut self, asset_id: AssetId) {
        if let Some(entry) = self.entries.get_mut(&asset_id) {
            entry.status = GpuResourceStatus::Unloaded;
            entry.size_bytes = 0;
        }
    }

    /// Remove a resource entry completely
    pub fn remove(&mut self, asset_id: AssetId) -> Option<GpuResourceEntry> {
        self.entries.remove(&asset_id)
    }

    /// Compute total GPU memory tracked across all `Ready` resources
    pub fn total_gpu_memory(&self) -> usize {
        self.entries
            .values()
            .filter(|e| e.status == GpuResourceStatus::Ready)
            .map(|e| e.size_bytes)
            .sum()
    }

    /// Count number of entries by status
    pub fn count_by_status(&self, status: GpuResourceStatus) -> usize {
        self.entries.values().filter(|e| e.status == status).count()
    }

    /// Get all entries
    pub fn entries(&self) -> impl Iterator<Item = &GpuResourceEntry> {
        self.entries.values()
    }

    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }

    pub fn record_cache_miss(&mut self) {
        self.cache_misses += 1;
    }

    pub fn record_reimport(&mut self) {
        self.reimport_count += 1;
    }

    pub fn record_uploaded_mesh(&mut self) {
        self.uploaded_meshes += 1;
    }

    pub fn record_uploaded_texture(&mut self) {
        self.uploaded_textures += 1;
    }

    pub fn record_uploaded_material(&mut self) {
        self.uploaded_materials += 1;
    }

    /// Retrieve snapshot of GPU resource statistics
    pub fn stats(&self) -> GpuResourceStats {
        GpuResourceStats {
            uploaded_meshes: self.uploaded_meshes,
            uploaded_textures: self.uploaded_textures,
            uploaded_materials: self.uploaded_materials,
            cache_hits: self.cache_hits,
            cache_misses: self.cache_misses,
            reimport_count: self.reimport_count,
            stale_count: self.count_by_status(GpuResourceStatus::Stale),
            approximate_memory_bytes: self.total_gpu_memory(),
        }
    }
}

/// Asset pipeline that integrates AssetDatabase, AssetStorage, and AssetCache for disk asset lifecycle
#[derive(Resource)]
pub struct AssetPipeline {
    pub database: AssetDatabase,
    pub storage: AssetStorage,
    pub cache: AssetCache,
}

impl AssetPipeline {
    pub fn new(database: AssetDatabase, storage: AssetStorage, cache: AssetCache) -> Self {
        Self {
            database,
            storage,
            cache,
        }
    }

    /// Get the asset database
    pub fn database(&self) -> &AssetDatabase {
        &self.database
    }

    /// Get mutable reference to the asset database
    pub fn database_mut(&mut self) -> &mut AssetDatabase {
        &mut self.database
    }

    /// Get the asset storage
    pub fn storage(&self) -> &AssetStorage {
        &self.storage
    }

    /// Get mutable reference to the asset storage
    pub fn storage_mut(&mut self) -> &mut AssetStorage {
        &mut self.storage
    }

    /// Get the asset cache
    pub fn cache(&self) -> &AssetCache {
        &self.cache
    }

    /// Get mutable reference to the asset cache
    pub fn cache_mut(&mut self) -> &mut AssetCache {
        &mut self.cache
    }

    /// Load an asset by ID
    pub fn load<T: Asset + AssetData>(&mut self, id: AssetId) -> Result<Handle<T>, String> {
        // Check if already in database
        if let Some(metadata) = self.database.registry.get(id) {
            if metadata.load_state == AssetLoadState::Loaded {
                return Ok(Handle::new(id));
            }
        }

        // Check cache first
        if let Some(data) = self.cache.get(id) {
            let _ = T::from_bytes(data)?;
            self.database
                .registry
                .set_load_state(id, AssetLoadState::Loaded);
            return Ok(Handle::new(id));
        }

        // Load from storage
        if self.storage.has_asset(id) {
            let data = self
                .storage
                .load_asset(id, T::asset_type())
                .map_err(|e| format!("Failed to load asset: {}", e))?;

            let _ = T::from_bytes(&data)?;

            // Cache the data
            self.cache.insert(id, data);
            self.database
                .registry
                .set_load_state(id, AssetLoadState::Loaded);

            return Ok(Handle::new(id));
        }

        Err(format!("Asset not found: {}", id))
    }

    /// Load an asset from source path
    pub fn load_from_path<T: Asset + AssetData>(
        &mut self,
        path: &PathBuf,
    ) -> Result<Handle<T>, String> {
        if let Some(id) = self.database.registry.find_by_source_path(path) {
            self.load(id)
        } else {
            Err(format!("Asset not registered at path: {}", path.display()))
        }
    }

    /// Get an asset by handle
    pub fn get<T: Asset + AssetData>(&self, handle: Handle<T>) -> Option<Arc<T>> {
        let id = handle.asset_id();
        if let Some(data) = self.cache.peek(id) {
            let asset_data = T::from_bytes(data).ok()?;
            Some(Arc::new(asset_data))
        } else if let Some(metadata) = self.database.registry.get(id) {
            if metadata.load_state == AssetLoadState::Loaded {
                let data = self.storage.load_asset(id, T::asset_type()).ok()?;
                let asset_data = T::from_bytes(&data).ok()?;
                Some(Arc::new(asset_data))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Register a new asset
    pub fn register<T: Asset + AssetData>(
        &mut self,
        name: String,
        asset: T,
        source_path: PathBuf,
    ) -> Handle<T> {
        let id = AssetId::new();
        let internal_path = self.storage.get_asset_path(id, T::asset_type());

        let metadata = crate::asset_database::AssetMetadata::new(
            id,
            name,
            T::asset_type(),
            source_path,
            internal_path,
        );

        let bytes = asset.to_bytes();

        // Store to disk
        self.storage
            .store_asset(id, T::asset_type(), &bytes)
            .expect("Failed to store asset");

        // Cache the data
        self.cache.insert(id, bytes);

        // Register in database
        self.database.registry.register(metadata, None);

        Handle::new(id)
    }

    /// Reload an asset
    pub fn reload<T: Asset + AssetData>(&mut self, handle: Handle<T>) -> Result<(), String> {
        let id = handle.asset_id();
        self.cache.remove(id);
        self.database
            .registry
            .set_load_state(id, AssetLoadState::NotLoaded);
        self.load::<T>(id)?;
        Ok(())
    }

    /// Unload an asset from memory (keeps it on disk)
    pub fn unload<T: Asset + AssetData>(&mut self, handle: Handle<T>) -> Result<(), String> {
        let id = handle.asset_id();
        self.cache.remove(id);
        self.database
            .registry
            .set_load_state(id, AssetLoadState::NotLoaded);
        Ok(())
    }

    /// Get all assets of a type
    pub fn get_all<T: Asset>(&self) -> Vec<Handle<T>> {
        let ids = self.database.registry.get_by_type(T::asset_type());
        ids.into_iter().map(Handle::new).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_database::{AssetDatabase, AssetId, AssetMetadata, AssetType};
    use crate::asset_storage::{AssetCache, AssetStorage, AssetStorageConfig};
    use tempfile::TempDir;

    #[test]
    fn asset_manager_basic() {
        let temp = TempDir::new().unwrap();
        let config = AssetStorageConfig {
            project_root: temp.path().to_path_buf(),
            imported_dir: temp.path().join("Assets/Imported"),
            cache_dir: temp.path().join("Assets/Cache"),
            max_cache_size: 0,
            importer_version: 1,
        };

        let database = AssetDatabase::new();
        let storage = AssetStorage::new(config).unwrap();
        let cache = AssetCache::new(1024 * 1024); // 1MB

        let mut manager = AssetPipeline::new(database, storage, cache);

        // Test registering an asset
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct TestAsset {
            value: u32,
        }

        impl Asset for TestAsset {
            fn asset_type() -> AssetType {
                AssetType::Other
            }
        }

        let asset = TestAsset { value: 42 };
        let handle = manager.register("test_asset".to_string(), asset, PathBuf::from("test.asset"));

        // Test loading the asset
        let loaded = manager.get::<TestAsset>(handle).unwrap();
        assert_eq!(loaded.value, 42);
    }

    #[test]
    fn gpu_resource_registry_lifecycle() {
        let mut registry = GpuResourceRegistry::new();
        let asset_id = AssetId::new();

        assert!(!registry.is_up_to_date(asset_id, 1));
        assert_eq!(registry.total_gpu_memory(), 0);

        // Upload revision 1
        registry.register_upload(asset_id, 1, GpuResourceKind::Mesh, 1001, 4096);
        assert!(registry.is_up_to_date(asset_id, 1));
        assert!(!registry.is_up_to_date(asset_id, 2));
        assert_eq!(registry.total_gpu_memory(), 4096);
        assert_eq!(
            registry.get(asset_id).unwrap().status,
            GpuResourceStatus::Ready
        );

        // Mark stale upon reimport
        registry.mark_stale(asset_id);
        assert!(!registry.is_up_to_date(asset_id, 1));
        assert_eq!(
            registry.get(asset_id).unwrap().status,
            GpuResourceStatus::Stale
        );

        // Re-upload with revision 2
        registry.register_upload(asset_id, 2, GpuResourceKind::Mesh, 1002, 8192);
        assert!(registry.is_up_to_date(asset_id, 2));
        assert_eq!(registry.total_gpu_memory(), 8192);

        // Unload
        registry.mark_unloaded(asset_id);
        assert!(!registry.is_up_to_date(asset_id, 2));
        assert_eq!(registry.total_gpu_memory(), 0);
        assert_eq!(
            registry.get(asset_id).unwrap().status,
            GpuResourceStatus::Unloaded
        );
    }

    #[test]
    fn asset_id_parse_and_metadata_revision() {
        let asset_id = AssetId::new();
        let mut meta = AssetMetadata::new(
            asset_id,
            "test_mesh".to_string(),
            AssetType::Mesh,
            PathBuf::from("models/test.glb"),
            PathBuf::from("cache/test.bin"),
        );

        assert_eq!(meta.revision, 0);
        let rev1 = meta.bump_revision();
        assert_eq!(rev1, 1);
        assert_eq!(meta.revision, 1);

        let id_str = meta.id.to_string();
        let parsed_id = AssetId::parse(&id_str);
        assert_eq!(parsed_id, Ok(meta.id));
        assert!(AssetId::parse("invalid-hex").is_err());
    }
}
