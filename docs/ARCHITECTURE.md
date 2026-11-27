# Rust Engine — Architecture

## Overview

Rust Engine is a native 3D engine and editor written in Rust. Stack: `winit` (window), `wgpu` 27 (rendering), `egui` 0.33 (UI, embedded in the winit/wgpu loop), `bevy_ecs` 0.16 (ECS), `glam` (math), `ron`+`serde` (scenes), `gltf` (import), `image` (textures).

Architecture: ECS (bevy_ecs) -> RenderSnapshot -> Renderer (wgpu). The renderer owns all GPU resources. ECS does not store low-level GPU objects, only `Handle<T>`.

## Data flow

```
ECS World
  └─ Renderable + GlobalTransform + Visible
        │
        ▼
collect_render_snapshot / collect_visible_renderables
        │
        ▼
RenderSnapshot (renderables, camera, directional_light)
        │
        ▼
Renderer::update (upload meshes/materials/textures on demand)
        │
        ▼
Frustum culling (render_scene / render_scene_to_viewport)
        │
        ▼
Main pass + UI pass
```

## Subsystems

### ECS / Components
- `Renderable { mesh: Handle<MeshAsset>, material: Handle<MaterialAsset> }` — component.
- `GlobalTransform`, `Transform`, `Visible`, `CameraMarker`, `Light`.
- `collect_render_snapshot` and `collect_visible_renderables` — gather renderable entities.

### Renderer (`src/renderer.rs`, ~3230 lines)
- `Renderer` owns: device, queue, surface, pipelines, asset managers, textures.
- `CameraUniform { view_proj, camera_pos }` — PBR-aligned (Phase 6.3).
- `ModelUniform { model, normal_matrix }`.
- `PbrMaterialUniform { base_color_factor, emissive_and_cutoff, pbr_factors, flags }`.
- `Vertex { position, color, normal, uv, tangent }` — full PBR layout (Phase 6.3).
- `MeshAsset { vertex_buffer, index_buffer, index_count, vertex_count, index_format, bounds, model_layout, model_buffers, model_bind_groups }`.
- `MaterialAsset { pipeline variants (opaque/alpha mask/alpha blend/double sided/MSAA/wireframe), material uniform buffer, 11-binding bind group }`.
- `TextureAsset { texture, view, sampler, bind_group, width, height, mip_levels, is_srgb, is_normal_map }`.
- `RenderableInstance { entity, renderable, global_transform }`.
- `RenderSnapshot { renderables, camera, directional_light }`.

### Frustum (`src/frustum.rs`)
- `Frustum::from_view_proj(&Mat4)`.
- `Frustum::intersects_sphere(center, radius)`.
- Integrated into `render_scene` and `render_scene_to_viewport`.
- Tests: inside/outside/intersecting/behind camera.

### Assets (`src/assets.rs`, `src/asset_storage.rs`, `src/asset_database.rs`, `src/asset_importer.rs`)
- `AssetManager<T, K>` — generic handle-based storage.
- `Handle<T>: Copy, Clone`.
- `GpuResourceRegistry` — tracks upload/cache/staleness.
- `sync_world_renderables` — on-demand mesh/material loading.

### glTF Import (`src/gltf_importer.rs`, ~995 lines)
- `MeshData`, `MeshBounds`, `MaterialData`, `TextureData`, `TextureSamplerSettings`, `TextureSemantic`, `AlphaMode`.

### Transforms (`src/transform.rs`)
- `Transform`, `GlobalTransform`, hierarchy.

### Camera (`src/camera.rs`)
- `Camera3D`.

### Scene (`src/scene.rs`)
- RON serialization/deserialization, UID.

### Editor (`src/editor.rs`, ~5883 lines)
- UI panels, workspaces, gizmos, selection, undo/redo.

### Viewport (`src/viewport.rs`)
- Offscreen texture, camera controls, render_scene_to_viewport.

### Performance (`src/performance.rs`, `src/performance_graph.rs`, `src/benchmark.rs`)
- Profiler, metrics, benchmark scenes.

## CPU vs GPU data

CPU: ECS components, MeshData/MeshBounds, MaterialData, TextureData, Transform, Scene.
GPU: Buffer, Texture, TextureView, Sampler, BindGroup, RenderPipeline, VertexBuffer/IndexBuffer.

## Current implementation limitations

- Shadow pass disabled in `render_scene()` (depth texture usage split comment). Shadow rendering NOT restored in Phase 6.3.
- Frustum culling integrated (Phase 8.1): `render_scene` and `render_scene_to_viewport` skip objects outside frustum. Shadow casters preserved (not culled).
- PBR production integration (Phase 6.3) complete: UV/tangent Vertex, camera_pos, PbrMaterialUniform, 11-binding material bind group, mip textures, sampler settings, fallback textures, alpha mask/blend, double sided, transparency sorting.

## Key files

- `src/renderer.rs` — renderer.
- `src/frustum.rs` — frustum.
- `src/assets.rs`, `src/asset_storage.rs`, `src/asset_database.rs`, `src/asset_importer.rs` — assets.
- `src/gltf_importer.rs` — import.
- `src/viewport.rs` — viewport.
- `src/app.rs` — application loop.
- `src/scene.rs` — scene.
- `src/editor.rs` — editor.
- `src/transform.rs` — transforms.
- `src/camera.rs` — camera.
- `src/performance.rs`, `src/benchmark.rs` — profiling.
