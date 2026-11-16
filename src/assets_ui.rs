//! Asset Editing workspace: data model and asset registry.
//!
//! The registry scans the project's `assets` folder and classifies every entry
//! into an [`AssetKind`]. The browser, preview and details panels are driven by
//! this data, so adding support for a new asset type only requires extending
//! [`AssetKind`] (plus the matching renderer in the preview/details panels).

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// High-level classification of a content entry. Drives which preview and
/// details UI are used, and which file extensions map to which kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Folder,
    Scene,
    Texture,
    Model,
    Material,
    Script,
    Shader,
    Audio,
    Unsupported,
}

impl AssetKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Folder => "Folder",
            Self::Scene => "Scene",
            Self::Texture => "Texture",
            Self::Model => "Model",
            Self::Material => "Material",
            Self::Script => "Script",
            Self::Shader => "Shader",
            Self::Audio => "Audio",
            Self::Unsupported => "Unsupported",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Folder => crate::icons::Icons::FOLDER,
            Self::Scene => crate::icons::Icons::SCENE,
            Self::Texture => crate::icons::Icons::TEXTURE,
            Self::Model => crate::icons::Icons::MODEL,
            Self::Material => crate::icons::Icons::MATERIAL,
            Self::Script => crate::icons::Icons::SCRIPT,
            Self::Shader => crate::icons::Icons::SHADER,
            Self::Audio => crate::icons::Icons::AUDIO,
            Self::Unsupported => crate::icons::Icons::FILE,
        }
    }
}

/// Rendering mode of the central asset preview viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewMode {
    Mesh,
    Texture,
    Material,
    Scene,
}

impl PreviewMode {
    pub const ALL: [PreviewMode; 4] = [
        PreviewMode::Mesh,
        PreviewMode::Texture,
        PreviewMode::Material,
        PreviewMode::Scene,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PreviewMode::Mesh => "3D Model",
            PreviewMode::Texture => "Texture",
            PreviewMode::Material => "Material",
            PreviewMode::Scene => "Scene",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            PreviewMode::Mesh => "Model Preview",
            PreviewMode::Texture => "Texture Preview",
            PreviewMode::Material => "Material Preview",
            PreviewMode::Scene => "Scene Preview",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            PreviewMode::Mesh => "Orbit, zoom, and inspect 3D geometry",
            PreviewMode::Texture => "Zoom, pan, and inspect pixel values",
            PreviewMode::Material => "Preview material on a sphere with parameters",
            PreviewMode::Scene => "Open scene in the main Scene Editor",
        }
    }
}

/// Classify a path by its extension. Directories are folders.
pub fn classify(path: &Path) -> AssetKind {
    if path.is_dir() {
        return AssetKind::Folder;
    }
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "ron" | "scene" | "lvl" | "umap" => AssetKind::Scene,
        "png" | "jpg" | "jpeg" | "bmp" | "tga" | "dds" | "hdr" | "exr" | "ktx" => {
            AssetKind::Texture
        }
        "obj" | "fbx" | "gltf" | "glb" | "dae" | "blend" | "3ds" | "msh" => AssetKind::Model,
        "mat" | "material" | "matx" => AssetKind::Material,
        "rs" | "lua" | "py" | "js" | "ts" => AssetKind::Script,
        "wgsl" | "hlsl" | "glsl" | "shader" | "spv" => AssetKind::Shader,
        "wav" | "mp3" | "ogg" | "flac" => AssetKind::Audio,
        _ => AssetKind::Unsupported,
    }
}

/// A single discovered entry in the content tree.
#[derive(Clone, Debug)]
pub struct AssetEntry {
    pub path: PathBuf,
    pub name: String,
    pub kind: AssetKind,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub modified: Option<SystemTime>,
}

impl AssetEntry {
    pub fn from_path(path: &Path) -> Option<Self> {
        let meta = fs::metadata(path).ok()?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let kind = classify(path);
        let size_bytes = meta.len();
        let modified = meta.modified().ok();
        Some(Self {
            path: path.to_path_buf(),
            name,
            kind,
            is_dir: meta.is_dir(),
            size_bytes,
            modified,
        })
    }

    /// Display extension without the leading dot, e.g. "obj", "png".
    pub fn extension(&self) -> String {
        self.path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    }

    /// Human-readable size, e.g. "4.2 KB".
    pub fn size_display(&self) -> String {
        format_bytes(self.size_bytes)
    }
}

/// Format a byte count with SI-ish units.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = UNITS[0];
    for &next in &UNITS[1..] {
        if size < 1024.0 {
            break;
        }
        size /= 1024.0;
        unit = next;
    }
    if unit == "B" {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {unit}")
    }
}

/// Format a system time as "YYYY-MM-DD HH:MM".
pub fn format_modified(time: SystemTime) -> String {
    let Ok(duration) = time.duration_since(SystemTime::UNIX_EPOCH) else {
        return "unknown".into();
    };
    let secs = duration.as_secs();
    let days = secs / 86400;
    let rem = secs % 86400;
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;

    // Civil date from days since epoch (Howard Hinnant's algorithm).
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02} {hour:02}:{minute:02}")
}

/// One entry of the folder tree, with enough info to render expand/collapse.
#[derive(Clone, Debug)]
pub struct FolderNode {
    pub path: PathBuf,
    pub name: String,
    pub children: Vec<FolderNode>,
}

/// Recursively collect folders under `dir` (depth-limited), preserving sort order.
pub fn build_folder_tree(root: &Path, max_depth: usize) -> FolderNode {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string());
    let mut node = FolderNode {
        path: root.to_path_buf(),
        name,
        children: Vec::new(),
    };
    if max_depth == 0 {
        return node;
    }
    let mut children: Vec<FolderNode> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            // Skip hidden / editor temp folders.
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !n.starts_with('.') && !n.starts_with('@'))
        })
        .map(|p| build_folder_tree(&p, max_depth - 1))
        .collect();
    children.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    node.children = children;
    node
}

/// Count asset files (not folders) recursively under `dir`.
pub fn count_assets(dir: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&current) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Minimal Wavefront OBJ parser: positions and face indices only. Good enough
/// for a preview mesh.
pub struct ObjMesh {
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl ObjMesh {
    /// Parse OBJ text. Returns `None` if no vertices/faces were found.
    pub fn parse(text: &str) -> Option<Self> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix('v') {
                if rest.starts_with(' ') {
                    let mut nums = rest.split_ascii_whitespace();
                    if let (Some(x), Some(y), Some(z)) = (
                        nums.next().and_then(|s| s.parse::<f32>().ok()),
                        nums.next().and_then(|s| s.parse::<f32>().ok()),
                        nums.next().and_then(|s| s.parse::<f32>().ok()),
                    ) {
                        vertices.push([x, y, z]);
                    }
                }
            } else if let Some(rest) = line.strip_prefix('f') {
                if rest.starts_with(' ') {
                    for token in rest.split_ascii_whitespace() {
                        let idx = token.split('/').next().unwrap_or("0");
                        if let Ok(i) = idx.parse::<i64>() {
                            let len = vertices.len() as i64;
                            let resolved = if i < 0 { len + i } else { i - 1 };
                            if resolved >= 0 && (resolved as usize) < vertices.len() {
                                indices.push(resolved as u32);
                            }
                        }
                    }
                }
            }
        }
        if vertices.is_empty() || indices.is_empty() {
            return None;
        }
        Some(Self { vertices, indices })
    }

    /// Number of triangles (assumes triangulated faces; larger polygons are
    /// fanned).
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Bounds of all vertices: (min, max).
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for v in &self.vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(v[axis]);
                max[axis] = max[axis].max(v[axis]);
            }
        }
        if !min[0].is_finite() {
            min = [0.0; 3];
            max = [0.0; 3];
        }
        (min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_extensions() {
        assert_eq!(classify(Path::new("a.png")), AssetKind::Texture);
        assert_eq!(classify(Path::new("a.OBJ")), AssetKind::Model);
        assert_eq!(classify(Path::new("a.mat")), AssetKind::Material);
        assert_eq!(classify(Path::new("a.ron")), AssetKind::Scene);
        assert_eq!(classify(Path::new("a.rs")), AssetKind::Script);
        assert_eq!(classify(Path::new("a.wgsl")), AssetKind::Shader);
        assert_eq!(classify(Path::new("a.unknown")), AssetKind::Unsupported);
        // Non-existent paths are never Folder (is_dir() returns false)
        assert_ne!(classify(Path::new("dir")), AssetKind::Folder);
        // Real directory is classified as Folder
        let temp = std::env::temp_dir().join("assets_ui_classify_test_dir");
        std::fs::create_dir_all(&temp).ok();
        assert_eq!(classify(&temp), AssetKind::Folder);
        let _ = std::fs::remove_dir(&temp);
    }

    #[test]
    fn parse_obj_mesh() {
        let text = "# cube\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let mesh = ObjMesh::parse(text).expect("mesh");
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.triangle_count(), 1);
        let (min, max) = mesh.bounds();
        assert_eq!(min, [0.0, 0.0, 0.0]);
        assert_eq!(max, [1.0, 1.0, 0.0]);
    }

    #[test]
    fn format_bytes_units() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn folder_tree_counts_assets() {
        let root = std::env::temp_dir().join(format!("engine_asset_tree_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("models")).unwrap();
        fs::write(root.join("models").join("cube.obj"), "v 0 0 0\n").unwrap();
        fs::write(root.join("readme.txt"), "hi").unwrap();
        let tree = build_folder_tree(&root, 4);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(count_assets(&root), 2);
        let _ = fs::remove_dir_all(&root);
    }
}
