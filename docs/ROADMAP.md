# Rust Engine — Roadmap

Statuses: COMPLETED / PARTIAL / SCAFFOLDING / PLANNED / NEEDS MANUAL VERIFICATION / BLOCKED.

## Phases

### Phase 0 — Foundation: COMPLETED
Base renderer, window, egui, ECS, transforms, scene, RON serialization.

### Phase 1 — Editor UI: COMPLETED
Docking, panels, workspaces, content browser, viewport, gizmos, selection.

### Phase 2 — Performance Monitor: COMPLETED (NEEDS MANUAL VERIFICATION)
CPU/GPU metrics, graphs. Audit defects fixed (8cd048b).

### Phase 3 — Undo/Redo: COMPLETED
Command system with history, History panel.

### Phase 4 — Asset Pipeline: COMPLETED (PARTIAL)
AssetManager, Handle, GpuResourceRegistry, glTF importer, texture upload (single mip, hardcoded sRGB).

### Phase 5 — Lighting/Shadows: PARTIAL
Directional light, shadow resources, but shadow pass disabled in render_scene(). Blocker: depth texture usage split.

### Phase 6 — PBR: COMPLETED (Phase 6.3)
PBR integrated into production renderer: UV/tangent Vertex, camera_pos, PbrMaterialUniform, 11-binding material bind group, mip textures, sampler settings, fallback textures, alpha mask/blend, double sided, transparency sorting. Shaders and tests aligned.

### Phase 6.1–6.2 — Texture Support: COMPLETED (Phase 6.3)
Mip chain, sRGB/linear views, sampler settings, material bind group 11 entries, texture dependency invalidation.

### Phase 6.3 — Production Renderer Reconciliation: COMPLETED
PBR integrated into production renderer. See Phase 6.3 spec for details.

### Phase 7 — Rendering Optimization: PARTIAL
Frustum culling integrated (Phase 8.1). Instancing — PLANNED (Phase 8.2).

### Phase 8.1 — Frustum Culling: COMPLETED
Frustum::from_view_proj, intersects_sphere. Integrated into render_scene and render_scene_to_viewport. Tests pass. Commit f97b5ae.

### Phase 8.2 — GPU Instancing: PLANNED

### Phase 9+ — Future
- Shadow rendering restore.
- Transparency sorting improvements.
- Post-processing pipeline.
- Scripting runtime.
- Physics.
- Audio.

## Current status (HEAD cf3e25d + Phase 6.3 changes)

- cargo test: 141 passed.
- cargo build --release: OK.
- cargo fmt --check: OK.
- git diff --check: OK.

## Blockers

1. Shadow pass disabled (depth texture usage split).
2. Instancing not implemented.

## Recommended next phase

Phase 5 — restore shadow rendering (depth attachment + sampling). Then Phase 8.2 (instancing).
