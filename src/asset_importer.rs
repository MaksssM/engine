use crate::asset_database::{AssetId, AssetLoadState, AssetMetadata, AssetType, ImportInfo};
use crate::asset_storage::AssetStorage;
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// Result of an import operation
#[derive(Debug, Clone)]
pub struct ImportResult {
    pub asset_id: AssetId,
    pub asset_type: AssetType,
    pub dependencies: Vec<AssetId>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Context passed to importers during import
pub struct ImportContext<'a> {
    pub asset_registry: &'a mut crate::asset_database::AssetRegistry,
    pub asset_storage: &'a mut AssetStorage,
    pub source_path: &'a Path,
    pub asset_id: AssetId,
    pub metadata: &'a mut AssetMetadata,
}

impl<'a> ImportContext<'a> {
    pub fn new(
        asset_registry: &'a mut crate::asset_database::AssetRegistry,
        asset_storage: &'a mut AssetStorage,
        source_path: &'a Path,
        asset_id: AssetId,
        metadata: &'a mut AssetMetadata,
    ) -> Self {
        Self {
            asset_registry,
            asset_storage,
            source_path,
            asset_id,
            metadata,
        }
    }

    /// Register a dependency asset
    pub fn register_dependency(&mut self, dependency_id: AssetId) {
        self.asset_registry
            .add_dependency(self.asset_id, dependency_id);
        self.metadata.dependencies.push(dependency_id);
    }

    /// Store imported asset data
    pub fn store_asset_data(&mut self, asset_type: AssetType, data: &[u8]) -> std::io::Result<()> {
        self.asset_storage
            .store_asset(self.asset_id, asset_type, data)
    }

    /// Add a warning
    pub fn warning(&mut self, msg: String) {
        self.metadata
            .import_info
            .as_mut()
            .map(|i| i.warnings.push(msg));
    }

    /// Add an error
    pub fn error(&mut self, msg: String) {
        self.metadata
            .import_info
            .as_mut()
            .map(|i| i.errors.push(msg));
    }
}

/// Base trait for all asset importers
pub trait AssetImporter: Send + Sync + 'static {
    /// Unique name of this importer
    fn name(&self) -> &'static str;

    /// Version of this importer
    fn version(&self) -> u32;

    /// File extensions this importer supports (without dot)
    fn extensions(&self) -> &'static [&'static str];

    /// Asset type this importer produces
    fn asset_type(&self) -> AssetType;

    /// Check if this importer can handle the given file
    fn can_import(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                self.extensions()
                    .contains(&ext.to_ascii_lowercase().as_str())
            })
            .unwrap_or(false)
    }

    /// Import the asset from the source file
    fn import(&self, context: &mut ImportContext) -> Result<ImportResult, ImportError>;

    /// Get supported asset types (for importers that produce multiple types)
    fn supported_types(&self) -> Vec<AssetType> {
        vec![self.asset_type()]
    }

    /// Called after all assets from a file are imported
    fn post_import(&self, _context: &mut ImportContext, _results: &[ImportResult]) {}
}

/// Error type for import operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportError {
    pub message: String,
    pub source_path: PathBuf,
    pub importer_name: String,
    pub recoverable: bool,
}

impl ImportError {
    pub fn new(message: impl Into<String>, source_path: &Path, importer_name: &str) -> Self {
        Self {
            message: message.into(),
            source_path: source_path.to_path_buf(),
            importer_name: importer_name.to_string(),
            recoverable: false,
        }
    }

    pub fn recoverable(
        message: impl Into<String>,
        source_path: &Path,
        importer_name: &str,
    ) -> Self {
        Self {
            message: message.into(),
            source_path: source_path.to_path_buf(),
            importer_name: importer_name.to_string(),
            recoverable: true,
        }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Import error in '{}' ({:?}): {}",
            self.importer_name, self.source_path, self.message
        )
    }
}

impl std::error::Error for ImportError {}

impl From<std::io::Error> for ImportError {
    fn from(err: std::io::Error) -> Self {
        ImportError::new(err.to_string(), Path::new(""), "io")
    }
}

/// Registry of all importers
pub struct ImporterRegistry {
    importers: Vec<Box<dyn AssetImporter>>,
    by_extension: std::collections::HashMap<String, usize>,
    by_type: std::collections::HashMap<AssetType, Vec<usize>>,
}

impl Default for ImporterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ImporterRegistry {
    pub fn new() -> Self {
        Self {
            importers: Vec::new(),
            by_extension: std::collections::HashMap::new(),
            by_type: std::collections::HashMap::new(),
        }
    }

    /// Register an importer
    pub fn register(&mut self, importer: Box<dyn AssetImporter>) {
        let index = self.importers.len();

        for ext in importer.extensions() {
            self.by_extension.insert(ext.to_lowercase(), index);
        }

        self.by_type
            .entry(importer.asset_type())
            .or_default()
            .push(index);

        self.importers.push(importer);
    }

    /// Find importer by file extension
    pub fn find_by_extension(&self, extension: &str) -> Option<&dyn AssetImporter> {
        self.by_extension
            .get(&extension.to_lowercase())
            .map(|&i| self.importers[i].as_ref())
    }

    /// Find importers by asset type
    pub fn find_by_type(&self, asset_type: AssetType) -> Vec<&dyn AssetImporter> {
        self.by_type
            .get(&asset_type)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&i| self.importers[i].as_ref())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all importers
    pub fn all(&self) -> Vec<&dyn AssetImporter> {
        self.importers.iter().map(|i| i.as_ref()).collect()
    }

    /// Import a file using the appropriate importer
    pub fn import_file(&self, context: &mut ImportContext) -> Result<ImportResult, ImportError> {
        let extension = context
            .source_path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| {
                ImportError::new("File has no extension", context.source_path, "unknown")
            })?;

        let importer = self.find_by_extension(extension).ok_or_else(|| {
            ImportError::new(
                format!("No importer for extension '.{}'", extension),
                context.source_path,
                "unknown",
            )
        })?;

        importer.import(context)
    }

    /// Get all supported extensions
    pub fn supported_extensions(&self) -> Vec<String> {
        self.by_extension.keys().cloned().collect()
    }
}

/// Import pipeline that manages the import process
pub struct ImportPipeline {
    pub registry: ImporterRegistry,
    pub asset_registry: crate::asset_database::AssetRegistry,
    pub asset_storage: AssetStorage,
}

impl ImportPipeline {
    pub fn new(
        asset_registry: crate::asset_database::AssetRegistry,
        asset_storage: AssetStorage,
    ) -> Self {
        Self {
            registry: ImporterRegistry::new(),
            asset_registry,
            asset_storage,
        }
    }

    /// Register an importer
    pub fn register_importer(&mut self, importer: Box<dyn AssetImporter>) {
        self.registry.register(importer);
    }

    /// Import a single file
    pub fn import_file(&mut self, source_path: &Path) -> Result<ImportResult, ImportError> {
        // Check if already imported
        if let Some(existing_id) = self.asset_registry.find_by_source_path(source_path) {
            let metadata = self.asset_registry.get_mut(existing_id).ok_or_else(|| {
                ImportError::new(
                    "Asset exists in registry but metadata missing",
                    source_path,
                    "import_pipeline",
                )
            })?;

            // Check if reimport needed
            if metadata.load_state == AssetLoadState::Loaded {
                return Err(ImportError::recoverable(
                    "Asset already imported",
                    source_path,
                    "import_pipeline",
                ));
            }
        }

        // Create new asset ID
        let asset_id = AssetId::new();
        let asset_type = self.determine_asset_type(source_path);

        let mut metadata = AssetMetadata::new(
            asset_id,
            source_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(),
            asset_type,
            source_path.to_path_buf(),
            Self::generate_internal_path(asset_id, asset_type),
        );

        metadata.load_state = AssetLoadState::Loading;

        // Create import context
        let mut context = ImportContext::new(
            &mut self.asset_registry,
            &mut self.asset_storage,
            source_path,
            asset_id,
            &mut metadata,
        );

        // Perform import
        let result = self.registry.import_file(&mut context)?;

        // Update metadata with result
        metadata.load_state = if result.errors.is_empty() {
            AssetLoadState::Loaded
        } else {
            AssetLoadState::Failed
        };
        metadata.import_info = Some(ImportInfo {
            importer_version: self.get_importer_version(source_path),
            source_hash: crate::asset_storage::compute_file_hash(source_path).unwrap_or_default(),
            import_time: std::time::SystemTime::now(),
            source_path: source_path.to_path_buf(),
            dependencies: result.dependencies.clone(),
            warnings: result.warnings.clone(),
            errors: result.errors.clone(),
        });
        metadata.dependencies = result.dependencies.clone();

        // Register in registry
        self.asset_registry.register(metadata, None);

        Ok(result)
    }

    /// Reimport an existing file, preserving its AssetId and updating its metadata and dependencies
    pub fn reimport_file(&mut self, source_path: &Path) -> Result<ImportResult, ImportError> {
        let (asset_id, asset_type, asset_name, internal_path, new_revision) =
            if let Some(existing_id) = self.asset_registry.find_by_source_path(source_path) {
                let meta = self.asset_registry.get(existing_id).ok_or_else(|| {
                    ImportError::new(
                        "Asset exists in registry but metadata missing",
                        source_path,
                        "import_pipeline",
                    )
                })?;
                (
                    existing_id,
                    meta.asset_type,
                    meta.name.clone(),
                    meta.internal_path.clone(),
                    meta.revision + 1,
                )
            } else {
                let id = AssetId::new();
                let t = self.determine_asset_type(source_path);
                let name = source_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                let int_p = Self::generate_internal_path(id, t);
                (id, t, name, int_p, 0)
            };

        // Remove stale sub-assets (meshes, materials, nodes, scenes) that were
        // generated by a previous import of this source file. This prevents
        // duplicate records from accumulating on every reimport.
        self.asset_registry.remove_all_by_source_path(source_path);

        let mut metadata = AssetMetadata::new(
            asset_id,
            asset_name,
            asset_type,
            source_path.to_path_buf(),
            internal_path,
        );
        metadata.revision = new_revision;
        metadata.load_state = AssetLoadState::Loading;

        let mut context = ImportContext::new(
            &mut self.asset_registry,
            &mut self.asset_storage,
            source_path,
            asset_id,
            &mut metadata,
        );

        let result = self.registry.import_file(&mut context)?;

        metadata.load_state = if result.errors.is_empty() {
            AssetLoadState::Loaded
        } else {
            AssetLoadState::Failed
        };
        metadata.import_info = Some(ImportInfo {
            importer_version: self.get_importer_version(source_path),
            source_hash: crate::asset_storage::compute_file_hash(source_path).unwrap_or_default(),
            import_time: std::time::SystemTime::now(),
            source_path: source_path.to_path_buf(),
            dependencies: result.dependencies.clone(),
            warnings: result.warnings.clone(),
            errors: result.errors.clone(),
        });
        metadata.dependencies = result.dependencies.clone();

        self.asset_registry.register(metadata, None);

        Ok(result)
    }

    /// Import a file that may produce multiple assets (e.g., glTF with meshes, materials, textures)
    pub fn import_file_multi(
        &mut self,
        source_path: &Path,
    ) -> Result<Vec<ImportResult>, ImportError> {
        let extension = source_path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| ImportError::new("File has no extension", source_path, "unknown"))?;

        let importer = self.registry.find_by_extension(extension).ok_or_else(|| {
            ImportError::new(
                format!("No importer for extension '.{}'", extension),
                source_path,
                "unknown",
            )
        })?;

        // For multi-asset imports, we need a different approach
        // This is a placeholder - actual implementation depends on importer
        let asset_id = AssetId::new();
        let asset_type = importer.asset_type();

        let mut metadata = AssetMetadata::new(
            asset_id,
            source_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(),
            asset_type,
            source_path.to_path_buf(),
            Self::generate_internal_path(asset_id, asset_type),
        );

        metadata.load_state = AssetLoadState::Loading;

        let mut context = ImportContext::new(
            &mut self.asset_registry,
            &mut self.asset_storage,
            source_path,
            asset_id,
            &mut metadata,
        );

        let result = importer.import(&mut context)?;

        metadata.load_state = if result.errors.is_empty() {
            AssetLoadState::Loaded
        } else {
            AssetLoadState::Failed
        };
        metadata.import_info = Some(ImportInfo {
            importer_version: importer.version(),
            source_hash: crate::asset_storage::compute_file_hash(source_path).unwrap_or_default(),
            import_time: std::time::SystemTime::now(),
            source_path: source_path.to_path_buf(),
            dependencies: result.dependencies.clone(),
            warnings: result.warnings.clone(),
            errors: result.errors.clone(),
        });
        metadata.dependencies = result.dependencies.clone();

        self.asset_registry.register(metadata, None);

        Ok(vec![result])
    }

    /// Determine asset type from file extension
    fn determine_asset_type(&self, path: &Path) -> AssetType {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if let Some(importer) = self.registry.find_by_extension(ext) {
                return importer.asset_type();
            }
        }
        AssetType::Other
    }

    /// Get importer version for a file
    fn get_importer_version(&self, path: &Path) -> u32 {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if let Some(importer) = self.registry.find_by_extension(ext) {
                return importer.version();
            }
        }
        0
    }

    /// Generate internal path for asset
    pub fn generate_internal_path(asset_id: AssetId, asset_type: AssetType) -> PathBuf {
        let type_dir = match asset_type {
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
        PathBuf::from(format!("{}/{}.asset", type_dir, asset_id))
    }

    /// Reimport an existing asset
    pub fn reimport(&mut self, asset_id: AssetId) -> Result<ImportResult, ImportError> {
        let metadata = self
            .asset_registry
            .get(asset_id)
            .ok_or_else(|| ImportError::new("Asset not found", Path::new(""), "import_pipeline"))?;

        let source_path = metadata.source_path.clone();
        self.import_file(&source_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn importer_registry_basic() {
        let registry = ImporterRegistry::new();

        // We can't easily test with a real importer without implementing one
        // This just tests the registry structure
        assert!(registry.all().is_empty());
        assert!(registry.find_by_extension("gltf").is_none());
    }

    #[test]
    fn import_error_display() {
        let err = ImportError::new("Test error", Path::new("test.gltf"), "TestImporter");
        let display = format!("{}", err);
        assert!(display.contains("TestImporter"));
        assert!(display.contains("test.gltf"));
        assert!(display.contains("Test error"));
    }

    #[test]
    fn reimport_preserves_asset_id_and_prevents_duplicates() {
        use crate::asset_database::AssetRegistry;

        let temp = TempDir::new().unwrap();
        let source = temp.path().join("model.gltf");
        // Write a minimal valid glTF document (non-indexed triangle).
        fs::write(
            &source,
            r#"{
                "asset": {"version": "2.0"},
                "scene": 0,
                "scenes": [{"nodes": [0]}],
                "nodes": [{"mesh": 0, "name": "Triangle"}],
                "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
                "buffers": [{"uri": "data:application/octet-stream;base64,AAAAAAAAgD8AAAAAAAAAQAAAQEAAAIA/AAAAQAAAAAAAAAAA", "byteLength": 36}],
                "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962}],
                "accessors": [{"bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0,0,0], "max": [1,1,1]}]
            }"#,
        )
        .unwrap();

        let mut config = crate::asset_storage::AssetStorageConfig::default();
        config.project_root = temp.path().to_path_buf();
        config.imported_dir = temp.path().join("Imported");
        config.cache_dir = temp.path().join("Cache");
        let storage = AssetStorage::new(config).unwrap();
        let mut pipeline =
            crate::asset_importer::ImportPipeline::new(AssetRegistry::new(), storage);
        pipeline.register_importer(Box::new(crate::gltf_importer::GltfImporter));

        // First import
        let first = pipeline.import_file(&source).unwrap();
        let first_id = first.asset_id;

        // Asset registered (model plus its sub-assets: meshes, nodes, scenes)
        let count_after_first = pipeline.asset_registry.len();
        assert!(count_after_first >= 1);
        assert!(pipeline.asset_registry.contains(first_id));
        assert_eq!(
            pipeline.asset_registry.find_by_source_path(&source),
            Some(first_id)
        );

        // Second import should be rejected (already imported)
        let second = pipeline.import_file(&source);
        assert!(second.is_err());

        // Reimport preserves the asset ID and does not create a duplicate
        let reimported = pipeline.reimport_file(&source).unwrap();
        assert_eq!(reimported.asset_id, first_id);
        assert_eq!(
            pipeline.asset_registry.len(),
            count_after_first,
            "reimport must not create duplicate asset records"
        );
    }
}
