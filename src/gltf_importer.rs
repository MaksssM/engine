//! glTF 2.0 Importer for Rust Engine
//!
//! Supports .gltf and .glb files with:
//! - Meshes (positions, normals, UVs, tangents, indices)
//! - Scene hierarchy (nodes, transforms)
//! - Materials (PBR metallic-roughness)
//! - Textures (embedded in GLB or external)

use crate::{
    asset_database::{AssetId, AssetLoadState, AssetMetadata, AssetType},
    asset_importer::{AssetImporter, ImportContext, ImportError, ImportResult},
};
use glam::{Mat4, Quat, Vec3};
use gltf::{Document, Gltf, mesh::Mode};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};

/// CPU-side mesh data
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshData {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    pub primitive_type: PrimitiveType,
    pub bounds: MeshBounds,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tex_coords: [f32; 2],
    pub tangent: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveType {
    Triangles,
    Lines,
    Points,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct MeshBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// CPU-side material data (PBR metallic-roughness)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaterialData {
    pub name: String,
    pub base_color_factor: [f32; 4],
    pub metallic_factor: f32,
    pub roughness_factor: f32,
    pub base_color_texture: Option<TextureRef>,
    pub metallic_roughness_texture: Option<TextureRef>,
    pub normal_texture: Option<TextureRef>,
    pub occlusion_texture: Option<TextureRef>,
    pub emissive_texture: Option<TextureRef>,
    pub emissive_factor: [f32; 3],
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
    pub double_sided: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextureRef {
    pub texture_id: AssetId,
    pub tex_coord: u32,
}

/// glTF wrapping mode (maps 1:1 onto wgpu address modes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TextureWrapMode {
    ClampToEdge,
    #[default]
    Repeat,
    MirroredRepeat,
}

/// glTF filter mode. `Nearest` covers the NEAREST family, `Linear` the LINEAR family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TextureFilterMode {
    Nearest,
    #[default]
    Linear,
}

/// glTF minification filter modes (glTF 2.0 constants 9728, 9729, 9984, 9985, 9986, 9987)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TextureMinFilter {
    Nearest,
    Linear,
    NearestMipmapNearest,
    LinearMipmapNearest,
    NearestMipmapLinear,
    #[default]
    LinearMipmapLinear,
}

impl TextureMinFilter {
    /// Returns (min_filter, mipmap_filter, lod_max_clamp) for wgpu sampler creation.
    pub fn to_wgpu(&self) -> (wgpu::FilterMode, wgpu::FilterMode, f32) {
        match self {
            TextureMinFilter::Nearest => {
                (wgpu::FilterMode::Nearest, wgpu::FilterMode::Nearest, 0.0)
            }
            TextureMinFilter::Linear => (wgpu::FilterMode::Linear, wgpu::FilterMode::Nearest, 0.0),
            TextureMinFilter::NearestMipmapNearest => {
                (wgpu::FilterMode::Nearest, wgpu::FilterMode::Nearest, 32.0)
            }
            TextureMinFilter::LinearMipmapNearest => {
                (wgpu::FilterMode::Linear, wgpu::FilterMode::Nearest, 32.0)
            }
            TextureMinFilter::NearestMipmapLinear => {
                (wgpu::FilterMode::Nearest, wgpu::FilterMode::Linear, 32.0)
            }
            TextureMinFilter::LinearMipmapLinear => {
                (wgpu::FilterMode::Linear, wgpu::FilterMode::Linear, 32.0)
            }
        }
    }

    /// Whether this filter samples mip levels > 0.
    pub fn is_mipmapped(&self) -> bool {
        !matches!(self, TextureMinFilter::Nearest | TextureMinFilter::Linear)
    }
}

/// Semantic role of a texture, used to guide mipmap filtering downsampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TextureSemantic {
    #[default]
    Color,
    Emissive,
    LinearData,
    NormalMap,
}

/// Sampler settings taken from the glTF texture. Defaults match the glTF spec
/// (repeat + linear) so assets serialized before sampler support still load.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextureSamplerSettings {
    pub wrap_s: TextureWrapMode,
    pub wrap_t: TextureWrapMode,
    pub mag_filter: TextureFilterMode,
    pub min_filter: TextureFilterMode,
    pub mipmap_filter: TextureFilterMode,
    #[serde(default)]
    pub min_filter_complete: Option<TextureMinFilter>,
}

impl Default for TextureSamplerSettings {
    fn default() -> Self {
        Self {
            wrap_s: TextureWrapMode::Repeat,
            wrap_t: TextureWrapMode::Repeat,
            mag_filter: TextureFilterMode::Linear,
            min_filter: TextureFilterMode::Linear,
            mipmap_filter: TextureFilterMode::Linear,
            min_filter_complete: Some(TextureMinFilter::LinearMipmapLinear),
        }
    }
}

impl TextureSamplerSettings {
    pub fn resolved_min_filter(&self) -> TextureMinFilter {
        if let Some(complete) = self.min_filter_complete {
            complete
        } else {
            match (self.min_filter, self.mipmap_filter) {
                (TextureFilterMode::Nearest, TextureFilterMode::Nearest) => {
                    TextureMinFilter::NearestMipmapNearest
                }
                (TextureFilterMode::Nearest, TextureFilterMode::Linear) => {
                    TextureMinFilter::NearestMipmapLinear
                }
                (TextureFilterMode::Linear, TextureFilterMode::Nearest) => {
                    TextureMinFilter::LinearMipmapNearest
                }
                (TextureFilterMode::Linear, TextureFilterMode::Linear) => {
                    TextureMinFilter::LinearMipmapLinear
                }
            }
        }
    }

    pub fn lod_max_clamp(&self) -> f32 {
        self.resolved_min_filter().to_wgpu().2
    }
}

/// CPU-side texture data
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextureData {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
    pub data: Vec<u8>,
    pub is_srgb: bool,
    /// glTF sampler. Absent in assets written before sampler support.
    #[serde(default)]
    pub sampler: TextureSamplerSettings,
    /// Texture semantic role hint for mipmap downsampling
    #[serde(default)]
    pub semantic: Option<TextureSemantic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureFormat {
    R8,
    Rg8,
    Rgb8,
    Rgba8,
    R16,
    Rg16,
    Rgb16,
    Rgba16,
    R32Float,
    Rg32Float,
    Rgb32Float,
    Rgba32Float,
}

/// CPU-side node data for scene hierarchy
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeData {
    pub name: String,
    pub transform: NodeTransform,
    pub mesh: Option<AssetId>,
    pub skin: Option<AssetId>,
    pub children: Vec<AssetId>,
    pub camera: Option<AssetId>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct NodeTransform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4], // quaternion x,y,z,w
    pub scale: [f32; 3],
    pub matrix: Option<[f32; 16]>,
}

impl NodeTransform {
    pub fn to_matrix(&self) -> Mat4 {
        if let Some(m) = self.matrix {
            Mat4::from_cols_array_2d(&[
                [m[0], m[1], m[2], m[3]],
                [m[4], m[5], m[6], m[7]],
                [m[8], m[9], m[10], m[11]],
                [m[12], m[13], m[14], m[15]],
            ])
        } else {
            let translation = Mat4::from_translation(Vec3::from(self.translation));
            let rotation = Mat4::from_quat(Quat::from_xyzw(
                self.rotation[0],
                self.rotation[1],
                self.rotation[2],
                self.rotation[3],
            ));
            let scale = Mat4::from_scale(Vec3::from(self.scale));
            translation * rotation * scale
        }
    }
}

/// glTF Importer
pub struct GltfImporter;

impl AssetImporter for GltfImporter {
    fn name(&self) -> &'static str {
        "gltf"
    }

    fn version(&self) -> u32 {
        1
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["gltf", "glb"]
    }

    fn asset_type(&self) -> AssetType {
        AssetType::Model
    }

    fn supported_types(&self) -> Vec<AssetType> {
        vec![
            AssetType::Model,
            AssetType::Mesh,
            AssetType::Material,
            AssetType::Texture,
        ]
    }

    fn import(&self, context: &mut ImportContext) -> Result<ImportResult, ImportError> {
        // Load glTF document with buffers and images
        let (document, buffers, images) = gltf::import(context.source_path)
            .map_err(|e| ImportError::new(e.to_string(), context.source_path, self.name()))?;

        let mut texture_ids = HashMap::new();
        let mut material_ids = HashMap::new();
        let mut mesh_ids = HashMap::new();
        let mut node_ids = HashMap::new();

        // Scan materials to determine texture semantic roles
        let mut normal_texture_indices = std::collections::HashSet::new();
        let mut data_texture_indices = std::collections::HashSet::new();
        for material in document.materials() {
            if let Some(tex) = material.normal_texture() {
                normal_texture_indices.insert(tex.texture().index());
            }
            let pbr = material.pbr_metallic_roughness();
            if let Some(tex) = pbr.metallic_roughness_texture() {
                data_texture_indices.insert(tex.texture().index());
            }
            if let Some(tex) = material.occlusion_texture() {
                data_texture_indices.insert(tex.texture().index());
            }
        }

        // 1. Import textures first (dependencies)
        for (index, texture) in document.textures().enumerate() {
            let semantic = if normal_texture_indices.contains(&index) {
                Some(TextureSemantic::NormalMap)
            } else if data_texture_indices.contains(&index) {
                Some(TextureSemantic::LinearData)
            } else {
                Some(TextureSemantic::Color)
            };
            let texture_id =
                self.import_texture(&document, &texture, context, index, &images, semantic)?;
            texture_ids.insert(index, texture_id);
        }

        // 2. Import materials
        for (index, material) in document.materials().enumerate() {
            let material_id =
                self.import_material(&document, &material, context, index, &texture_ids)?;
            material_ids.insert(index, material_id);
        }

        // 3. Import meshes
        for (index, mesh) in document.meshes().enumerate() {
            let mesh_id =
                self.import_mesh(&document, &mesh, context, index, &material_ids, &buffers)?;
            mesh_ids.insert(index, mesh_id);
        }

        // 4. Pre-allocate node IDs so children can be resolved regardless of node order
        for (index, _) in document.nodes().enumerate() {
            node_ids.insert(index, AssetId::new());
        }

        // Import nodes (scene hierarchy)
        for (index, node) in document.nodes().enumerate() {
            let node_id = *node_ids.get(&index).unwrap();
            self.import_node(
                &document, &node, context, index, node_id, &mesh_ids, &node_ids,
            )?;
        }

        // 5. Import scenes
        for (index, scene) in document.scenes().enumerate() {
            self.import_scene(&document, &scene, context, index, &node_ids)?;
        }

        // Collect all dependencies
        let mut dependencies = Vec::new();
        dependencies.extend(texture_ids.values().copied());
        dependencies.extend(material_ids.values().copied());
        dependencies.extend(mesh_ids.values().copied());
        dependencies.extend(node_ids.values().copied());

        // Return result for the main model asset
        let model_id = context.asset_id;
        Ok(ImportResult {
            asset_id: model_id,
            asset_type: AssetType::Model,
            dependencies,
            warnings: vec![],
            errors: vec![],
        })
    }
}

impl GltfImporter {
    /// Load glTF document from file (handles both .gltf and .glb)
    fn load_gltf_document(path: &Path) -> Result<Document, Box<dyn std::error::Error>> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext.to_ascii_lowercase().as_str() {
            "glb" => {
                let data = std::fs::read(path)?;
                let gltf = Gltf::from_slice(&data)?;
                Ok(gltf.document)
            }
            "gltf" => {
                let gltf = Gltf::open(path)?;
                Ok(gltf.document)
            }
            _ => Err("Unsupported file extension".into()),
        }
    }

    fn import_texture(
        &self,
        _document: &Document,
        texture: &gltf::Texture,
        context: &mut ImportContext,
        index: usize,
        images: &[gltf::image::Data],
        semantic: Option<TextureSemantic>,
    ) -> Result<AssetId, ImportError> {
        let source = texture.source();
        let image_index = source.index();
        let image = &images[image_index];

        let texture_id = AssetId::new();
        let name = texture
            .name()
            .unwrap_or(&format!("texture_{}", index))
            .to_string();

        let (width, height, format, data, is_srgb) = self.load_image_data(image)?;

        let sampler = texture.sampler();
        let wrap = |mode: gltf::texture::WrappingMode| match mode {
            gltf::texture::WrappingMode::ClampToEdge => TextureWrapMode::ClampToEdge,
            gltf::texture::WrappingMode::MirroredRepeat => TextureWrapMode::MirroredRepeat,
            gltf::texture::WrappingMode::Repeat => TextureWrapMode::Repeat,
        };
        let min_filter_complete = match sampler.min_filter() {
            Some(gltf::texture::MinFilter::Nearest) => TextureMinFilter::Nearest,
            Some(gltf::texture::MinFilter::Linear) => TextureMinFilter::Linear,
            Some(gltf::texture::MinFilter::NearestMipmapNearest) => {
                TextureMinFilter::NearestMipmapNearest
            }
            Some(gltf::texture::MinFilter::LinearMipmapNearest) => {
                TextureMinFilter::LinearMipmapNearest
            }
            Some(gltf::texture::MinFilter::NearestMipmapLinear) => {
                TextureMinFilter::NearestMipmapLinear
            }
            Some(gltf::texture::MinFilter::LinearMipmapLinear) => {
                TextureMinFilter::LinearMipmapLinear
            }
            None => TextureMinFilter::LinearMipmapLinear,
        };
        let min_filter = match sampler.min_filter() {
            Some(gltf::texture::MinFilter::Nearest)
            | Some(gltf::texture::MinFilter::NearestMipmapNearest)
            | Some(gltf::texture::MinFilter::NearestMipmapLinear) => TextureFilterMode::Nearest,
            _ => TextureFilterMode::Linear,
        };
        let mipmap_filter = match sampler.min_filter() {
            Some(gltf::texture::MinFilter::NearestMipmapNearest)
            | Some(gltf::texture::MinFilter::LinearMipmapNearest) => TextureFilterMode::Nearest,
            _ => TextureFilterMode::Linear,
        };
        let mag_filter = match sampler.mag_filter() {
            Some(gltf::texture::MagFilter::Nearest) => TextureFilterMode::Nearest,
            _ => TextureFilterMode::Linear,
        };

        let texture_data = TextureData {
            name: name.clone(),
            width,
            height,
            format,
            data,
            is_srgb,
            sampler: TextureSamplerSettings {
                wrap_s: wrap(sampler.wrap_s()),
                wrap_t: wrap(sampler.wrap_t()),
                mag_filter,
                min_filter,
                mipmap_filter,
                min_filter_complete: Some(min_filter_complete),
            },
            semantic,
        };

        // Serialize and store
        let serialized = ron::to_string(&texture_data)
            .map_err(|e| ImportError::new(e.to_string(), context.source_path, self.name()))?;

        context.store_asset_data(AssetType::Texture, serialized.as_bytes())?;

        // Register in registry
        let mut metadata = AssetMetadata::new(
            texture_id,
            name,
            AssetType::Texture,
            context.source_path.to_path_buf(),
            context
                .asset_storage
                .get_asset_path(texture_id, AssetType::Texture),
        );
        metadata.load_state = AssetLoadState::Loaded;
        context
            .asset_registry
            .register(metadata, Some(Box::new(texture_data)));

        Ok(texture_id)
    }

    fn load_image_data(
        &self,
        image: &gltf::image::Data,
    ) -> Result<(u32, u32, TextureFormat, Vec<u8>, bool), ImportError> {
        let format = match image.format {
            gltf::image::Format::R8 => TextureFormat::R8,
            gltf::image::Format::R8G8 => TextureFormat::Rg8,
            gltf::image::Format::R8G8B8 => TextureFormat::Rgb8,
            gltf::image::Format::R8G8B8A8 => TextureFormat::Rgba8,
            gltf::image::Format::R16 => TextureFormat::R16,
            gltf::image::Format::R16G16 => TextureFormat::Rg16,
            gltf::image::Format::R16G16B16 => TextureFormat::Rgb16,
            gltf::image::Format::R16G16B16A16 => TextureFormat::Rgba16,
            gltf::image::Format::R32G32B32FLOAT => TextureFormat::Rgb32Float,
            gltf::image::Format::R32G32B32A32FLOAT => TextureFormat::Rgba32Float,
        };

        let is_srgb = matches!(
            image.format,
            gltf::image::Format::R8G8B8 | gltf::image::Format::R8G8B8A8
        );

        Ok((
            image.width,
            image.height,
            format,
            image.pixels.clone(),
            is_srgb,
        ))
    }

    fn import_material(
        &self,
        _document: &Document,
        material: &gltf::Material,
        context: &mut ImportContext,
        index: usize,
        texture_ids: &HashMap<usize, AssetId>,
    ) -> Result<AssetId, ImportError> {
        let material_id = AssetId::new();
        let name = material
            .name()
            .unwrap_or(&format!("material_{}", index))
            .to_string();

        let pbr = material.pbr_metallic_roughness();

        let base_color_factor = pbr.base_color_factor();
        let metallic_factor = pbr.metallic_factor();
        let roughness_factor = pbr.roughness_factor();

        let base_color_texture = pbr.base_color_texture().map(|tex| {
            let tex_index = tex.texture().index();
            TextureRef {
                texture_id: *texture_ids.get(&tex_index).unwrap(),
                tex_coord: tex.tex_coord(),
            }
        });

        let metallic_roughness_texture = pbr.metallic_roughness_texture().map(|tex| {
            let tex_index = tex.texture().index();
            TextureRef {
                texture_id: *texture_ids.get(&tex_index).unwrap(),
                tex_coord: tex.tex_coord(),
            }
        });

        let normal_texture = material.normal_texture().map(|tex| {
            let tex_index = tex.texture().index();
            TextureRef {
                texture_id: *texture_ids.get(&tex_index).unwrap(),
                tex_coord: tex.tex_coord(),
            }
        });

        let occlusion_texture = material.occlusion_texture().map(|tex| {
            let tex_index = tex.texture().index();
            TextureRef {
                texture_id: *texture_ids.get(&tex_index).unwrap(),
                tex_coord: tex.tex_coord(),
            }
        });

        let emissive_texture = material.emissive_texture().map(|tex| {
            let tex_index = tex.texture().index();
            TextureRef {
                texture_id: *texture_ids.get(&tex_index).unwrap(),
                tex_coord: tex.tex_coord(),
            }
        });

        let emissive_factor = material.emissive_factor();
        let alpha_mode = match material.alpha_mode() {
            gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
            gltf::material::AlphaMode::Mask => AlphaMode::Mask,
            gltf::material::AlphaMode::Blend => AlphaMode::Blend,
        };
        let alpha_cutoff = material.alpha_cutoff().unwrap_or(0.5);
        let double_sided = material.double_sided();

        let material_data = MaterialData {
            name: name.clone(),
            base_color_factor,
            metallic_factor,
            roughness_factor,
            base_color_texture,
            metallic_roughness_texture,
            normal_texture,
            occlusion_texture,
            emissive_texture,
            emissive_factor,
            alpha_mode,
            alpha_cutoff,
            double_sided,
        };

        let serialized = ron::to_string(&material_data)
            .map_err(|e| ImportError::new(e.to_string(), context.source_path, self.name()))?;

        context.store_asset_data(AssetType::Material, serialized.as_bytes())?;

        let mut metadata = AssetMetadata::new(
            material_id,
            name,
            AssetType::Material,
            context.source_path.to_path_buf(),
            context
                .asset_storage
                .get_asset_path(material_id, AssetType::Material),
        );
        metadata.load_state = AssetLoadState::Loaded;
        context
            .asset_registry
            .register(metadata, Some(Box::new(material_data)));

        Ok(material_id)
    }

    fn import_mesh(
        &self,
        _document: &Document,
        mesh: &gltf::Mesh,
        context: &mut ImportContext,
        index: usize,
        _material_ids: &HashMap<usize, AssetId>,
        buffers: &[gltf::buffer::Data],
    ) -> Result<AssetId, ImportError> {
        let mesh_id = AssetId::new();
        let name = mesh
            .name()
            .unwrap_or(&format!("mesh_{}", index))
            .to_string();

        let mut all_vertices = Vec::new();
        let mut all_indices = Vec::new();
        let mut primitive_type = PrimitiveType::Triangles;
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);

        for primitive in mesh.primitives() {
            // Check mode
            match primitive.mode() {
                Mode::Triangles => primitive_type = PrimitiveType::Triangles,
                Mode::TriangleStrip | Mode::TriangleFan => {
                    primitive_type = PrimitiveType::Triangles
                }
                Mode::Lines | Mode::LineStrip | Mode::LineLoop => {
                    primitive_type = PrimitiveType::Lines
                }
                Mode::Points => primitive_type = PrimitiveType::Points,
            }

            // Read attributes
            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()].0));

            let positions: Vec<[f32; 3]> = reader
                .read_positions()
                .map(|iter| iter.collect())
                .unwrap_or_default();
            let normals: Vec<[f32; 3]> = reader
                .read_normals()
                .map(|iter| iter.collect())
                .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);
            let tex_coords: Vec<[f32; 2]> = reader
                .read_tex_coords(0)
                .map(|iter| iter.into_f32().collect())
                .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);
            let tangents: Vec<[f32; 4]> = reader
                .read_tangents()
                .map(|iter| iter.collect())
                .unwrap_or_else(|| vec![[1.0, 0.0, 0.0, 1.0]; positions.len()]);

            // Update bounds
            for pos in &positions {
                let p = Vec3::from(*pos);
                min = min.min(p);
                max = max.max(p);
            }

            let vertex_offset = all_vertices.len() as u32;
            for i in 0..positions.len() {
                all_vertices.push(MeshVertex {
                    position: positions[i],
                    normal: normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
                    tex_coords: tex_coords.get(i).copied().unwrap_or([0.0, 0.0]),
                    tangent: tangents.get(i).copied().unwrap_or([1.0, 0.0, 0.0, 1.0]),
                });
            }

            // Read indices
            if let Some(indices) = reader.read_indices() {
                for idx in indices.into_u32() {
                    all_indices.push(vertex_offset + idx);
                }
            } else {
                // Generate indices for non-indexed geometry
                for i in 0..positions.len() {
                    all_indices.push(vertex_offset + i as u32);
                }
            }
        }

        let mesh_data = MeshData {
            vertices: all_vertices,
            indices: all_indices,
            primitive_type,
            bounds: MeshBounds {
                min: min.into(),
                max: max.into(),
            },
            name: name.clone(),
        };

        let serialized = ron::to_string(&mesh_data)
            .map_err(|e| ImportError::new(e.to_string(), context.source_path, self.name()))?;

        context.store_asset_data(AssetType::Mesh, serialized.as_bytes())?;

        let mut metadata = AssetMetadata::new(
            mesh_id,
            name,
            AssetType::Mesh,
            context.source_path.to_path_buf(),
            context
                .asset_storage
                .get_asset_path(mesh_id, AssetType::Mesh),
        );
        metadata.load_state = AssetLoadState::Loaded;
        context
            .asset_registry
            .register(metadata, Some(Box::new(mesh_data)));

        Ok(mesh_id)
    }

    fn import_node(
        &self,
        _document: &Document,
        node: &gltf::Node,
        context: &mut ImportContext,
        index: usize,
        node_id: AssetId,
        mesh_ids: &HashMap<usize, AssetId>,
        node_ids: &HashMap<usize, AssetId>,
    ) -> Result<AssetId, ImportError> {
        let name = node
            .name()
            .unwrap_or(&format!("node_{}", index))
            .to_string();

        // Get transform
        let transform = match node.transform() {
            gltf::scene::Transform::Matrix { matrix } => NodeTransform {
                translation: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
                matrix: Some(glam::Mat4::from_cols_array_2d(&matrix).to_cols_array()),
            },
            gltf::scene::Transform::Decomposed {
                translation,
                rotation,
                scale,
            } => NodeTransform {
                translation,
                rotation: [rotation[0], rotation[1], rotation[2], rotation[3]],
                scale,
                matrix: None,
            },
        };

        // Get mesh reference
        let mesh = node.mesh().and_then(|m| mesh_ids.get(&m.index()).copied());

        // Get children
        let mut children = Vec::new();
        for child in node.children() {
            if let Some(child_id) = node_ids.get(&child.index()) {
                children.push(*child_id);
            }
        }

        let node_data = NodeData {
            name: name.clone(),
            transform,
            mesh,
            skin: None, // Not supported yet
            children,
            camera: None, // Not supported yet
        };

        let serialized = ron::to_string(&node_data)
            .map_err(|e| ImportError::new(e.to_string(), context.source_path, self.name()))?;

        context.store_asset_data(AssetType::Model, serialized.as_bytes())?;

        let mut metadata = AssetMetadata::new(
            node_id,
            name,
            AssetType::Model,
            context.source_path.to_path_buf(),
            context
                .asset_storage
                .get_asset_path(node_id, AssetType::Model),
        );
        metadata.load_state = AssetLoadState::Loaded;
        context
            .asset_registry
            .register(metadata, Some(Box::new(node_data)));

        Ok(node_id)
    }

    fn import_scene(
        &self,
        _document: &Document,
        scene: &gltf::Scene,
        context: &mut ImportContext,
        index: usize,
        node_ids: &HashMap<usize, AssetId>,
    ) -> Result<AssetId, ImportError> {
        let scene_id = AssetId::new();
        let name = scene
            .name()
            .unwrap_or(&format!("scene_{}", index))
            .to_string();

        let mut root_nodes = Vec::new();
        for node in scene.nodes() {
            if let Some(node_id) = node_ids.get(&node.index()) {
                root_nodes.push(*node_id);
            }
        }

        // Store scene data
        let scene_data = SceneData {
            name: name.clone(),
            root_nodes,
        };

        let serialized = ron::to_string(&scene_data)
            .map_err(|e| ImportError::new(e.to_string(), context.source_path, self.name()))?;

        context.store_asset_data(AssetType::Scene, serialized.as_bytes())?;

        let mut metadata = AssetMetadata::new(
            scene_id,
            name,
            AssetType::Scene,
            context.source_path.to_path_buf(),
            context
                .asset_storage
                .get_asset_path(scene_id, AssetType::Scene),
        );
        metadata.load_state = AssetLoadState::Loaded;
        context
            .asset_registry
            .register(metadata, Some(Box::new(scene_data)));

        Ok(scene_id)
    }
}

/// CPU-side scene data
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SceneData {
    pub name: String,
    pub root_nodes: Vec<AssetId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset_database::AssetRegistry;
    use crate::asset_storage::AssetStorage;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn gltf_importer_extensions() {
        let importer = GltfImporter;
        assert!(importer.can_import(Path::new("test.gltf")));
        assert!(importer.can_import(Path::new("test.glb")));
        assert!(!importer.can_import(Path::new("test.obj")));
    }

    fn test_pipeline(root: &Path) -> crate::asset_importer::ImportPipeline {
        let mut config = crate::asset_storage::AssetStorageConfig::default();
        config.project_root = root.to_path_buf();
        config.imported_dir = root.join("Imported");
        config.cache_dir = root.join("Cache");
        let storage = AssetStorage::new(config).unwrap();
        let mut pipeline =
            crate::asset_importer::ImportPipeline::new(AssetRegistry::new(), storage);
        pipeline.register_importer(Box::new(GltfImporter));
        pipeline
    }

    #[test]
    fn gltf_importer_missing_file_returns_error() {
        let importer = GltfImporter;
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join("does_not_exist.gltf");
        let mut pipeline = test_pipeline(temp.path());
        let mut metadata = AssetMetadata::new(
            AssetId::new(),
            "missing".to_string(),
            AssetType::Model,
            missing.clone(),
            PathBuf::from("imported/model"),
        );
        metadata.load_state = AssetLoadState::Loading;
        let mut context = ImportContext::new(
            &mut pipeline.asset_registry,
            &mut pipeline.asset_storage,
            &missing,
            metadata.id,
            &mut metadata,
        );
        let result = importer.import(&mut context);
        assert!(result.is_err(), "missing file must produce an import error");
    }

    #[test]
    fn gltf_importer_corrupted_file_returns_error() {
        let importer = GltfImporter;
        let temp = TempDir::new().unwrap();
        let corrupted = temp.path().join("corrupt.gltf");
        fs::write(&corrupted, b"this is not a valid gltf document").unwrap();
        let mut pipeline = test_pipeline(temp.path());
        let mut metadata = AssetMetadata::new(
            AssetId::new(),
            "corrupt".to_string(),
            AssetType::Model,
            corrupted.clone(),
            PathBuf::from("imported/model"),
        );
        metadata.load_state = AssetLoadState::Loading;
        let mut context = ImportContext::new(
            &mut pipeline.asset_registry,
            &mut pipeline.asset_storage,
            &corrupted,
            metadata.id,
            &mut metadata,
        );
        let result = importer.import(&mut context);
        assert!(
            result.is_err(),
            "corrupted gltf must produce an import error"
        );
    }

    #[test]
    fn gltf_importer_imports_minimal_gltf() {
        let temp = TempDir::new().unwrap();
        let gltf_path = temp.path().join("triangle.gltf");

        // Minimal valid glTF 2.0: one non-indexed triangle with POSITION accessor.
        // Buffer holds 3 vec3 positions (36 bytes).
        let gltf_json = r#"{
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0, "name": "Triangle"}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
            "buffers": [{"uri": "data:application/octet-stream;base64,AAAAAAAAgD8AAAAAAAAAQAAAQEAAAIA/AAAAQAAAAAAAAAAA", "byteLength": 36}],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962}
            ],
            "accessors": [
                {"bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0,0,0], "max": [1,1,1]}
            ]
        }"#;
        fs::write(&gltf_path, gltf_json).unwrap();

        let mut pipeline = test_pipeline(temp.path());
        let import = pipeline.import_file(&gltf_path).unwrap();
        assert_eq!(import.asset_type, AssetType::Model);
        assert!(
            import.errors.is_empty(),
            "minimal gltf should import without errors: {:?}",
            import.errors
        );
        assert!(
            import.dependencies.iter().any(|d| d != &import.asset_id),
            "imported model should register dependencies for meshes"
        );
    }

    #[test]
    fn texture_sampler_filter_modes_map_correctly() {
        let filters = [
            (
                TextureMinFilter::Nearest,
                wgpu::FilterMode::Nearest,
                wgpu::FilterMode::Nearest,
                false,
            ),
            (
                TextureMinFilter::Linear,
                wgpu::FilterMode::Linear,
                wgpu::FilterMode::Nearest,
                false,
            ),
            (
                TextureMinFilter::NearestMipmapNearest,
                wgpu::FilterMode::Nearest,
                wgpu::FilterMode::Nearest,
                true,
            ),
            (
                TextureMinFilter::LinearMipmapNearest,
                wgpu::FilterMode::Linear,
                wgpu::FilterMode::Nearest,
                true,
            ),
            (
                TextureMinFilter::NearestMipmapLinear,
                wgpu::FilterMode::Nearest,
                wgpu::FilterMode::Linear,
                true,
            ),
            (
                TextureMinFilter::LinearMipmapLinear,
                wgpu::FilterMode::Linear,
                wgpu::FilterMode::Linear,
                true,
            ),
        ];

        for (filter, expected_min, expected_mip, expected_is_mipmapped) in filters {
            let (min_f, mip_f, _) = filter.to_wgpu();
            assert_eq!(min_f, expected_min);
            assert_eq!(mip_f, expected_mip);
            assert_eq!(filter.is_mipmapped(), expected_is_mipmapped);
        }
    }

    #[test]
    fn texture_sampler_settings_resolves_min_filter_and_lod() {
        let sampler_no_mips = TextureSamplerSettings {
            wrap_s: TextureWrapMode::Repeat,
            wrap_t: TextureWrapMode::Repeat,
            mag_filter: TextureFilterMode::Linear,
            min_filter: TextureFilterMode::Linear,
            mipmap_filter: TextureFilterMode::Nearest,
            min_filter_complete: Some(TextureMinFilter::Linear),
        };
        assert_eq!(
            sampler_no_mips.resolved_min_filter(),
            TextureMinFilter::Linear
        );
        assert_eq!(sampler_no_mips.lod_max_clamp(), 0.0);

        let sampler_with_mips = TextureSamplerSettings {
            wrap_s: TextureWrapMode::ClampToEdge,
            wrap_t: TextureWrapMode::ClampToEdge,
            mag_filter: TextureFilterMode::Linear,
            min_filter: TextureFilterMode::Linear,
            mipmap_filter: TextureFilterMode::Linear,
            min_filter_complete: Some(TextureMinFilter::LinearMipmapLinear),
        };
        assert_eq!(
            sampler_with_mips.resolved_min_filter(),
            TextureMinFilter::LinearMipmapLinear
        );
        assert_eq!(sampler_with_mips.lod_max_clamp(), 32.0);
    }
}
