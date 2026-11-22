use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    hash::Hash,
    path::{Path, PathBuf},
    time::SystemTime,
};
use uuid::Uuid;

use crate::assets::AssetData;

/// Unique, stable identifier for an asset. Does not depend on memory location or entity IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId(pub Uuid);

impl AssetId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn nil() -> Self {
        Self(Uuid::nil())
    }

    pub fn from_u64_pair(high: u64, low: u64) -> Self {
        Self(Uuid::from_u64_pair(high, low))
    }

    pub fn from_u64(id: u64) -> Self {
        Self(Uuid::from_u64_pair(id, 0))
    }

    pub fn as_u64_pair(&self) -> (u64, u64) {
        self.0.as_u64_pair()
    }

    pub fn as_u64(&self) -> u64 {
        let (high, _) = self.0.as_u64_pair();
        high
    }

    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(Self)
    }
}

impl Default for AssetId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type of an asset
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetType {
    Mesh,
    Material,
    Texture,
    Model,
    Shader,
    Scene,
    Audio,
    Script,
    Other,
}

impl AssetType {
    pub fn label(&self) -> &'static str {
        match self {
            AssetType::Mesh => "Mesh",
            AssetType::Material => "Material",
            AssetType::Texture => "Texture",
            AssetType::Model => "Model",
            AssetType::Shader => "Shader",
            AssetType::Scene => "Scene",
            AssetType::Audio => "Audio",
            AssetType::Script => "Script",
            AssetType::Other => "Other",
        }
    }
}

/// Current loading state of an asset
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetLoadState {
    NotLoaded,
    Loading,
    Loaded,
    Failed,
    NeedsReimport,
}

/// Information about the import process for an asset
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportInfo {
    pub importer_version: u32,
    pub source_hash: String,
    pub import_time: SystemTime,
    pub source_path: PathBuf,
    pub dependencies: Vec<AssetId>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Metadata for an asset in the registry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetMetadata {
    pub id: AssetId,
    pub name: String,
    pub asset_type: AssetType,
    pub source_path: PathBuf,
    pub internal_path: PathBuf,
    pub load_state: AssetLoadState,
    pub import_info: Option<ImportInfo>,
    pub dependencies: Vec<AssetId>,
    pub dependents: Vec<AssetId>,
    pub tags: HashSet<String>,
    #[serde(default)]
    pub revision: u64,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

impl AssetMetadata {
    pub fn new(
        id: AssetId,
        name: String,
        asset_type: AssetType,
        source_path: PathBuf,
        internal_path: PathBuf,
    ) -> Self {
        let now = SystemTime::now();
        Self {
            id,
            name,
            asset_type,
            source_path,
            internal_path,
            load_state: AssetLoadState::NotLoaded,
            import_info: None,
            dependencies: Vec::new(),
            dependents: Vec::new(),
            tags: HashSet::new(),
            revision: 0,
            created_at: now,
            updated_at: now,
        }
    }

    /// Increment the asset revision and update timestamp.
    pub fn bump_revision(&mut self) -> u64 {
        self.revision += 1;
        self.updated_at = SystemTime::now();
        self.revision
    }
}

/// A stored asset with its metadata and CPU-side data
#[derive(Debug)]
pub struct StoredAsset {
    pub metadata: AssetMetadata,
    pub data: Option<Box<dyn AssetData>>,
}

/// Central registry for all assets in the project
#[derive(Default)]
pub struct AssetRegistry {
    assets: HashMap<AssetId, StoredAsset>,
    by_name: HashMap<String, AssetId>,
    by_source_path: HashMap<PathBuf, AssetId>,
    by_internal_path: HashMap<PathBuf, AssetId>,
    by_type: HashMap<AssetType, HashSet<AssetId>>,
}

impl AssetRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new asset or update existing one
    pub fn register(
        &mut self,
        metadata: AssetMetadata,
        data: Option<Box<dyn AssetData>>,
    ) -> AssetId {
        let id = metadata.id;

        // Remove old entries if updating
        if let Some(old) = self.assets.get(&id) {
            self.by_name.remove(&old.metadata.name);
            self.by_source_path.remove(&old.metadata.source_path);
            self.by_internal_path.remove(&old.metadata.internal_path);
            self.by_type
                .entry(old.metadata.asset_type)
                .or_default()
                .remove(&id);
        }

        self.by_name.insert(metadata.name.clone(), id);
        self.by_source_path.insert(metadata.source_path.clone(), id);
        self.by_internal_path
            .insert(metadata.internal_path.clone(), id);
        self.by_type
            .entry(metadata.asset_type)
            .or_default()
            .insert(id);

        let stored = StoredAsset { metadata, data };
        self.assets.insert(id, stored);
        id
    }

    /// Check if registry contains asset by ID
    pub fn contains(&self, id: AssetId) -> bool {
        self.assets.contains_key(&id)
    }

    /// Get asset metadata by ID
    pub fn get(&self, id: AssetId) -> Option<&AssetMetadata> {
        self.assets.get(&id).map(|a| &a.metadata)
    }

    /// Get mutable asset metadata by ID
    pub fn get_mut(&mut self, id: AssetId) -> Option<&mut AssetMetadata> {
        self.assets.get_mut(&id).map(|a| &mut a.metadata)
    }

    /// Get asset data by ID
    pub fn get_data<T: AssetData>(&self, id: AssetId) -> Option<&T> {
        self.assets
            .get(&id)
            .and_then(|a| a.data.as_ref())
            .and_then(|d| d.as_any().downcast_ref::<T>())
    }

    /// Get mutable asset data by ID
    pub fn get_data_mut<T: AssetData>(&mut self, id: AssetId) -> Option<&mut T> {
        self.assets
            .get_mut(&id)
            .and_then(|a| a.data.as_mut())
            .and_then(|d| d.as_any_mut().downcast_mut::<T>())
    }

    /// Find asset by name
    pub fn find_by_name(&self, name: &str) -> Option<AssetId> {
        self.by_name.get(name).copied()
    }

    /// Find asset by source path
    pub fn find_by_source_path(&self, path: &Path) -> Option<AssetId> {
        self.by_source_path.get(path).copied()
    }

    /// Find asset by internal path
    pub fn find_by_internal_path(&self, path: &Path) -> Option<AssetId> {
        self.by_internal_path.get(path).copied()
    }

    /// Get all assets of a specific type
    pub fn get_by_type(&self, asset_type: AssetType) -> Vec<AssetId> {
        self.by_type
            .get(&asset_type)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Get all asset IDs
    pub fn all_ids(&self) -> Vec<AssetId> {
        self.assets.keys().copied().collect()
    }

    /// Check if asset exists
    pub fn exists(&self, id: AssetId) -> bool {
        self.assets.contains_key(&id)
    }

    /// Update asset load state
    pub fn set_load_state(&mut self, id: AssetId, state: AssetLoadState) {
        if let Some(asset) = self.assets.get_mut(&id) {
            asset.metadata.load_state = state;
            asset.metadata.updated_at = SystemTime::now();
        }
    }

    /// Add dependency
    pub fn add_dependency(&mut self, asset: AssetId, dependency: AssetId) {
        if let Some(a) = self.assets.get_mut(&asset) {
            if !a.metadata.dependencies.contains(&dependency) {
                a.metadata.dependencies.push(dependency);
            }
        }
        if let Some(d) = self.assets.get_mut(&dependency) {
            if !d.metadata.dependents.contains(&asset) {
                d.metadata.dependents.push(asset);
            }
        }
    }

    /// Remove asset from registry
    pub fn remove(&mut self, id: AssetId) -> Option<StoredAsset> {
        if let Some(asset) = self.assets.remove(&id) {
            self.by_name.remove(&asset.metadata.name);
            self.by_source_path.remove(&asset.metadata.source_path);
            self.by_internal_path.remove(&asset.metadata.internal_path);
            self.by_type
                .entry(asset.metadata.asset_type)
                .or_default()
                .remove(&id);
            // Remove from dependents' dependency lists
            for dep_id in &asset.metadata.dependencies {
                if let Some(dep) = self.assets.get_mut(dep_id) {
                    dep.metadata.dependents.retain(|d| *d != id);
                }
            }
            // Remove from dependencies' dependent lists
            for dep_id in &asset.metadata.dependents {
                if let Some(dep) = self.assets.get_mut(dep_id) {
                    dep.metadata.dependencies.retain(|d| *d != id);
                }
            }
            Some(asset)
        } else {
            None
        }
    }

    /// Remove asset by source path
    pub fn remove_by_source_path(&mut self, source_path: &Path) -> Option<StoredAsset> {
        if let Some(id) = self.by_source_path.get(source_path).copied() {
            self.remove(id)
        } else {
            None
        }
    }

    /// Remove every asset whose source path matches, returning the removed assets.
    /// Used during reimport to clear stale sub-assets (meshes, materials, etc.)
    /// before regenerating them, preventing duplicate records.
    pub fn remove_all_by_source_path(&mut self, source_path: &Path) -> Vec<StoredAsset> {
        let ids: Vec<AssetId> = self
            .assets
            .iter()
            .filter(|(_, a)| a.metadata.source_path == source_path)
            .map(|(id, _)| *id)
            .collect();
        let mut removed = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(asset) = self.remove(id) {
                removed.push(asset);
            }
        }
        removed
    }

    /// Validate that all source files exist; marks missing ones as failed
    pub fn validate_source_files(&mut self) -> Vec<(AssetId, PathBuf)> {
        let mut missing = Vec::new();
        for (id, asset) in &mut self.assets {
            if !asset.metadata.source_path.exists() {
                asset.metadata.load_state = AssetLoadState::Failed;
                missing.push((*id, asset.metadata.source_path.clone()));
            }
        }
        missing
    }

    /// Get all assets that depend on this asset
    pub fn get_dependents(&self, id: AssetId) -> Vec<AssetId> {
        self.assets
            .get(&id)
            .map(|a| a.metadata.dependents.clone())
            .unwrap_or_default()
    }

    /// Get all dependencies of this asset
    pub fn get_dependencies(&self, id: AssetId) -> Vec<AssetId> {
        self.assets
            .get(&id)
            .map(|a| a.metadata.dependencies.clone())
            .unwrap_or_default()
    }

    /// Get count of assets
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    /// Iterate over all assets
    pub fn iter(&self) -> impl Iterator<Item = (&AssetId, &AssetMetadata)> {
        self.assets.iter().map(|(id, a)| (id, &a.metadata))
    }

    /// Iterate over all assets with data
    pub fn iter_with_data(
        &self,
    ) -> impl Iterator<Item = (&AssetId, &AssetMetadata, &dyn AssetData)> {
        self.assets
            .iter()
            .filter_map(|(id, a)| a.data.as_ref().map(|d| (id, &a.metadata, d.as_ref())))
    }
}

/// Resource to hold the asset registry in the ECS
use bevy_ecs::resource::Resource;

#[derive(Resource, Default)]
pub struct AssetDatabase {
    pub registry: AssetRegistry,
}

/// Events for asset changes
#[derive(Debug, Clone)]
pub enum AssetEvent {
    Added(AssetId),
    Updated(AssetId),
    Removed(AssetId),
    LoadStateChanged(AssetId, AssetLoadState),
    ReimportRequested(AssetId),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerializedAssetDatabase {
    pub assets: Vec<AssetMetadata>,
}

impl AssetDatabase {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        let serialized = SerializedAssetDatabase {
            assets: self.registry.iter().map(|(_, m)| m.clone()).collect(),
        };
        let ron_str = ron::ser::to_string_pretty(&serialized, ron::ser::PrettyConfig::default())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, ron_str)
    }

    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let serialized: SerializedAssetDatabase =
            ron::from_str(&content).map_err(|e| e.to_string())?;
        let mut db = Self::new();
        for meta in serialized.assets {
            db.registry.register(meta, None);
        }
        Ok(db)
    }

    pub fn load_or_create(path: &Path) -> Self {
        Self::load_from_file(path).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn asset_id_uniqueness() {
        let id1 = AssetId::new();
        let id2 = AssetId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn asset_registry_register_and_find() {
        let mut registry = AssetRegistry::new();
        let id = AssetId::new();
        let metadata = AssetMetadata::new(
            id,
            "test_mesh".to_string(),
            AssetType::Mesh,
            PathBuf::from("assets/models/test.gltf"),
            PathBuf::from("imported/meshes/test.mesh"),
        );
        registry.register(metadata, None);

        assert_eq!(registry.find_by_name("test_mesh"), Some(id));
        assert_eq!(
            registry.find_by_source_path(Path::new("assets/models/test.gltf")),
            Some(id)
        );
        assert_eq!(
            registry.find_by_internal_path(Path::new("imported/meshes/test.mesh")),
            Some(id)
        );
        assert_eq!(registry.get_by_type(AssetType::Mesh), vec![id]);
    }

    #[test]
    fn asset_registry_dependencies() {
        let mut registry = AssetRegistry::new();
        let mesh_id = AssetId::new();
        let material_id = AssetId::new();

        let mesh_meta = AssetMetadata::new(
            mesh_id,
            "mesh".to_string(),
            AssetType::Mesh,
            PathBuf::from("a.gltf"),
            PathBuf::from("mesh.mesh"),
        );
        let material_meta = AssetMetadata::new(
            material_id,
            "material".to_string(),
            AssetType::Material,
            PathBuf::from("a.gltf"),
            PathBuf::from("material.mat"),
        );

        registry.register(mesh_meta, None);
        registry.register(material_meta, None);
        registry.add_dependency(mesh_id, material_id);

        assert_eq!(registry.get_dependencies(mesh_id), vec![material_id]);
        assert_eq!(registry.get_dependents(material_id), vec![mesh_id]);
    }

    #[test]
    fn asset_id_persistence_file_round_trip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("asset_database.ron");

        let mut db = AssetDatabase::new();
        let id1 = AssetId::new();
        let id2 = AssetId::new();

        let meta1 = AssetMetadata::new(
            id1,
            "mesh_helmet".to_string(),
            AssetType::Mesh,
            PathBuf::from("assets/models/helmet.glb"),
            PathBuf::from("imported/meshes/helmet.mesh"),
        );
        let meta2 = AssetMetadata::new(
            id2,
            "mat_helmet".to_string(),
            AssetType::Material,
            PathBuf::from("assets/materials/helmet.mat"),
            PathBuf::from("imported/materials/helmet.mat"),
        );

        db.registry.register(meta1, None);
        db.registry.register(meta2, None);

        // Save
        db.save_to_file(&db_path)
            .expect("Failed to save asset database");
        assert!(db_path.exists());

        // Load
        let loaded_db =
            AssetDatabase::load_from_file(&db_path).expect("Failed to load asset database");
        assert_eq!(loaded_db.registry.len(), 2);
        assert_eq!(loaded_db.registry.find_by_name("mesh_helmet"), Some(id1));
        assert_eq!(loaded_db.registry.find_by_name("mat_helmet"), Some(id2));
        assert_eq!(
            loaded_db
                .registry
                .find_by_source_path(Path::new("assets/models/helmet.glb")),
            Some(id1)
        );
        assert_eq!(
            loaded_db
                .registry
                .find_by_source_path(Path::new("assets/materials/helmet.mat")),
            Some(id2)
        );

        let retrieved1 = loaded_db.registry.get(id1).expect("id1 should exist");
        assert_eq!(retrieved1.name, "mesh_helmet");
        assert_eq!(retrieved1.asset_type, AssetType::Mesh);
        assert_eq!(
            retrieved1.internal_path,
            PathBuf::from("imported/meshes/helmet.mesh")
        );
    }

    #[test]
    fn duplicate_import_prevention_and_reimport() {
        let mut registry = AssetRegistry::new();
        let source_path = PathBuf::from("assets/models/car.glb");

        // First import
        let original_id = AssetId::new();
        let meta = AssetMetadata::new(
            original_id,
            "car".to_string(),
            AssetType::Model,
            source_path.clone(),
            PathBuf::from("imported/models/car.asset"),
        );
        registry.register(meta, None);

        // Simulated reimport: lookup by source path to reuse existing ID
        let found_id = registry.find_by_source_path(&source_path);
        assert_eq!(
            found_id,
            Some(original_id),
            "Must find existing AssetId for duplicate source file"
        );

        // Update metadata using existing ID
        let mut updated_meta = AssetMetadata::new(
            found_id.unwrap(),
            "car_updated".to_string(),
            AssetType::Model,
            source_path.clone(),
            PathBuf::from("imported/models/car.asset"),
        );
        updated_meta.load_state = AssetLoadState::Loaded;
        registry.register(updated_meta, None);

        assert_eq!(
            registry.len(),
            1,
            "Should not create duplicate records for same source file"
        );
        let current = registry.get(original_id).unwrap();
        assert_eq!(current.name, "car_updated");
        assert_eq!(current.load_state, AssetLoadState::Loaded);
    }

    #[test]
    fn missing_source_files_detection() {
        let temp_dir = tempfile::tempdir().unwrap();
        let existing_path = temp_dir.path().join("exists.png");
        std::fs::write(&existing_path, b"dummy").unwrap();

        let missing_path = temp_dir.path().join("deleted.png");

        let mut registry = AssetRegistry::new();
        let id_ok = AssetId::new();
        let id_missing = AssetId::new();

        registry.register(
            AssetMetadata::new(
                id_ok,
                "exists".to_string(),
                AssetType::Texture,
                existing_path,
                PathBuf::from("imported/textures/exists.tex"),
            ),
            None,
        );
        registry.register(
            AssetMetadata::new(
                id_missing,
                "missing".to_string(),
                AssetType::Texture,
                missing_path,
                PathBuf::from("imported/textures/missing.tex"),
            ),
            None,
        );

        let missing_report = registry.validate_source_files();
        assert_eq!(missing_report.len(), 1);
        assert_eq!(missing_report[0].0, id_missing);

        let missing_asset = registry.get(id_missing).unwrap();
        assert_eq!(missing_asset.load_state, AssetLoadState::Failed);
    }

    #[test]
    fn remove_by_source_path() {
        let mut registry = AssetRegistry::new();
        let source_path = PathBuf::from("assets/textures/grass.png");
        let id = AssetId::new();

        registry.register(
            AssetMetadata::new(
                id,
                "grass".to_string(),
                AssetType::Texture,
                source_path.clone(),
                PathBuf::from("imported/textures/grass.tex"),
            ),
            None,
        );

        assert!(registry.contains(id));
        let removed = registry.remove_by_source_path(&source_path);
        assert!(removed.is_some());
        assert!(!registry.contains(id));
        assert_eq!(registry.find_by_source_path(&source_path), None);
    }
}
