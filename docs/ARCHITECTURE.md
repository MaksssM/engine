# Rust Engine — Архитектура

## Обзор

Rust Engine — нативный 3D-движок и редактор на Rust. Стек: `winit` (окно), `wgpu` 27 (рендер), `egui` 0.33 (UI, встроенный в winit/wgpu цикл), `bevy_ecs` 0.16 (ECS), `glam` (математика), `ron`+`serde` (сцены), `gltf` (импорт), `image` (текстуры).

Архитектура: ECS (bevy_ecs) → RenderSnapshot → Renderer (wgpu). Рендерер владеет всеми GPU-ресурсами. ECS не хранит низкоуровневые GPU-объекты, только `Handle<T>`.

## Поток данных

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

## Основные подсистемы

### ECS / Компоненты
- `Renderable { mesh: Handle<MeshAsset>, material: Handle<MaterialAsset> }` — компонент.
- `GlobalTransform`, `Transform`, `Visible`, `CameraMarker`, `Light`.
- `collect_render_snapshot` и `collect_visible_renderables` — сбор рендерабельных сущностей.

### Рендерер (`src/renderer.rs`, ~3230 строк)
- `Renderer` владеет: device, queue, surface, pipelines, asset managers, текстуры.
- `CameraUniform { view_proj }` — базовый (упрощённый).
- `ModelUniform { model, normal_matrix }`.
- `MaterialUniform { color }` — упрощённый (один цвет).
- `Vertex { position, color, normal }` — упрощённый (без UV/tangent).
- `MeshAsset { vertex_buffer, index_buffer, index_count, vertex_count, index_format, bounds, model_layout, model_buffers, model_bind_groups }`.
- `MaterialAsset { pipeline_* (1x/4x, wireframe), color_buffer, color_bind_group, color_bind_group_layout }`.
- `TextureAsset { texture, view, sampler, bind_group, width, height }`.
- `RenderableInstance { entity, renderable, global_transform }`.
- `RenderSnapshot { renderables, camera, directional_light }`.

### Frustum (`src/frustum.rs`)
- `Frustum::from_view_proj(&Mat4)`.
- `Frustum::intersects_sphere(center, radius)`.
- Интегрирован в `render_scene` и `render_scene_to_viewport`.
- Тесты: inside/outside/intersecting/behind camera.

### Ассеты (`src/assets.rs`, `src/asset_storage.rs`, `src/asset_database.rs`, `src/asset_importer.rs`)
- `AssetManager<T, K>` — generic handle-based storage.
- `Handle<T>: Copy, Clone`.
- `GpuResourceRegistry` — отслеживание загрузки/кэша/устаревания.
- `sync_world_renderables` — загрузка мешей/материалов по требованию.

### Импорт glTF (`src/gltf_importer.rs`, ~995 строк)
- `MeshData`, `MeshBounds`, `MaterialData`, `TextureData`, `TextureSamplerSettings`, `TextureSemantic`, `AlphaMode`.

### Трансформы (`src/transform.rs`)
- `Transform`, `GlobalTransform`, иерархия.

### Камера (`src/camera.rs`)
- `Camera3D`.

### Сцена (`src/scene.rs`)
- Сериализация/десериализация RON, UID.

### Редактор (`src/editor.rs`, ~5883 строк)
- UI панели, workspaces, gizmos, selection, undo/redo.

### Viewport (`src/viewport.rs`)
- Offscreen texture, camera controls, render_scene_to_viewport.

### Производительность (`src/performance.rs`, `src/performance_graph.rs`, `src/benchmark.rs`)
- Профилировщик, метрики, бенчмарк-сцены.

## CPU vs GPU данные

CPU: ECS компоненты, MeshData/MeshBounds, MaterialData, TextureData, Transform, Scene.
GPU: Buffer, Texture, TextureView, Sampler, BindGroup, RenderPipeline, VertexBuffer/IndexBuffer.

## Ограничения текущей реализации

- Рендерер использует упрощённые структуры (`CameraUniform` без позиции камеры, `MaterialUniform` только цвет, `Vertex` без UV/tangent). PBR-пайплайн (UV, tangent, 11-binding material layout) описан в задачах Phase 6.3, но в текущем коде НЕ интегрирован в production.
- Shadow pass отключён в `render_scene()` (комментарий про depth texture usage split).
- Текстуры: одиночный mip level, hardcoded sRGB, default sampler.
- Материалы: один binding (color), нет текстурных ссылок.
- Frustum culling интегрирован (Phase 8.1): `render_scene` и `render_scene_to_viewport` пропускают объекты вне фрустума. Shadow casters сохраняются (не куллятся).

## Ключевые файлы

- `src/renderer.rs` — рендерер.
- `src/frustum.rs` — фрустум.
- `src/assets.rs`, `src/asset_storage.rs`, `src/asset_database.rs`, `src/asset_importer.rs` — ассеты.
- `src/gltf_importer.rs` — импорт.
- `src/viewport.rs` — вьюпорт.
- `src/app.rs` — цикл приложения.
- `src/scene.rs` — сцена.
- `src/editor.rs` — редактор.
- `src/transform.rs` — трансформы.
- `src/camera.rs` — камера.
- `src/performance.rs`, `src/benchmark.rs` — профилирование.
