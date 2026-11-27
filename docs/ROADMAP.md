# Rust Engine — Roadmap

Статусы: COMPLETED / PARTIAL / SCAFFOLDING / PLANNED / NEEDS MANUAL VERIFICATION / BLOCKED.

## Фазы

### Phase 0 — Foundation: COMPLETED
Базовый рендерер, окно, egui, ECS, трансформы, сцена, сериализация RON.

### Phase 1 — Editor UI: COMPLETED
Docking, панели, workspaces, content browser, viewport, gizmos, selection.

### Phase 2 — Performance Monitor: COMPLETED (NEEDS MANUAL VERIFICATION)
Метрики CPU/GPU, графики. Аудит дефектов исправлен (8cd048b).

### Phase 3 — Undo/Redo: COMPLETED
Система команд с историей, панель History.

### Phase 4 — Asset Pipeline: COMPLETED (PARTIAL)
AssetManager, Handle, GpuResourceRegistry, glTF importer, texture upload (одиночный mip, sRGB hardcoded).

### Phase 5 — Lighting/Shadows: PARTIAL
Directional light, shadow resources, но shadow pass отключён в render_scene(). Блокер: depth texture usage split.

### Phase 6 — PBR: SCAFFOLDING (NOT INTEGRATED)
Шейдеры (shader_standard/fast/quality) и тесты продвинулись, но production renderer использует упрощённые структуры (CameraUniform без позиции камеры, MaterialUniform только цвет, Vertex без UV/tangent). Material bind group = 1 binding. Texture upload без mip chain.

### Phase 6.1–6.2: PLANNED (NOT DONE IN PRODUCTION)
Mip chain, sRGB/linear views, sampler settings, material bind group 11 entries, texture dependency invalidation.

### Phase 6.3 — Production Renderer Reconciliation: PLANNED
Интеграция PBR в production renderer. См. спецификацию Phase 6.3.

### Phase 7 — Rendering Optimization: PARTIAL
Frustum culling интегрирован (Phase 8.1). Instancing — PLANNED (Phase 8.2).

### Phase 8.1 — Frustum Culling: COMPLETED
Frustum::from_view_proj, intersects_sphere. Интегрирован в render_scene и render_scene_to_viewport. Тесты проходят. Commit f97b5ae.

### Phase 8.2 — GPU Instancing: PLANNED

### Phase 9+ — Future
- Shadow rendering restore.
- PBR production integration.
- Transparency sorting.
- Post-processing pipeline.
- Scripting runtime.
- Physics.
- Audio.

## Текущий статус (HEAD f97b5ae)

- cargo test: 141 passed (на момент Phase 8.1).
- cargo build --release: OK.
- cargo fmt --check: OK.
- git diff --check: OK.

## Блокеры

1. PBR не интегрирован в production renderer (упрощённые структуры).
2. Shadow pass отключён.
3. Texture upload без mip chain.
4. Material bind group 1 binding вместо 11.

## Следующая фаза (рекомендация)

Phase 6.3 — Production Renderer Reconciliation: интегрировать PBR (UV, tangent, camera_pos, 11-binding material, mip textures) в production renderer. Затем Phase 5 (shadows restore), затем Phase 8.2 (instancing).
