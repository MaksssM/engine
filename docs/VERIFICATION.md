# Rust Engine — Verification

## Automated checks

Run before finishing a task (when applicable):

```powershell
cargo fmt --all -- --check
cargo check
cargo test
cargo build --release
git diff --check
```

Do not claim tests passed if not run. Do not claim the app ran if not run.

## Manual editor checklist

1. Launch: `cargo run` — window opens without wgpu validation errors.
2. Open PBR Validation Scene (`assets/scenes/pbr_validation.ron`).
3. Confirm sphere geometry renders (not default cube).
4. Verify all nine materials (red plastic, polished metal, rough metal, matte dielectric, normal mapped, emissive, alpha mask, alpha blend, double sided).
5. Verify metallic/roughness differences.
6. Verify normal mapping.
7. Verify emissive material.
8. Verify Alpha Mask and Alpha Blend.
9. Verify Double-Sided.
10. Verify Viewport and Asset Preview.
11. Reimport a model and its texture.
12. Save and reload the scene.

## PBR verification (Phase 6.3)

- CameraUniform layout matches WGSL (view_proj + camera_pos).
- Vertex layout: position, color, normal, uv, tangent.
- PbrMaterialUniform: base color, emissive, alpha cutoff, metallic, roughness, texture flags, alpha mode, double-sided.
- 11-binding material bind group layout.
- upload_mesh preserves uv/tangent.
- upload_texture: mip chain, sRGB/linear views, sampler settings.
- Material bind group caching + invalidation on texture reimport.
- Pipeline variants: double-sided, alpha mask, alpha blend, MSAA, wireframe.
- Offscreen PBR rendering with pixel output check.

## Performance benchmark (Phase 7/8)

Scenarios: 100 / 1000 / 10000 objects. Distribution: all visible, mostly outside frustum, around camera. Metrics: candidate renderables, visible, culled, culling time, render prep time, CPU/GPU frame time, draw calls. Measure before and after.

## Limitations

- GPU tests, manual editor checks, and visual checks are not considered done until actually performed.
- If impossible, state the reason.
- Current HEAD: cf3e25d + Phase 6.3 changes. Shadow pass disabled. PBR integrated into production.
