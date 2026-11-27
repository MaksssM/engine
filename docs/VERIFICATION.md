# Rust Engine — Verification

## Автоматические проверки

Перед завершением задачи запускать (если применимо):

```powershell
cargo fmt --all -- --check
cargo check
cargo test
cargo build --release
git diff --check
```

Не утверждать, что тесты пройдены, если не запускались. Не утверждать, что приложение запускалось, если не запускалось.

## Ручной чек-лист редактора

1. Запуск: `cargo run` — окно открывается без wgpu validation errors.
2. Открыть PBR Validation Scene (`assets/scenes/pbr_validation.ron`).
3. Подтвердить рендер сферы (не куба по умолчанию).
4. Проверить 9 материалов (red plastic, polished metal, rough metal, matte dielectric, normal mapped, emissive, alpha mask, alpha blend, double sided).
5. Проверить metallic/roughness различия.
6. Проверить normal mapping.
7. Проверить emissive материал.
8. Проверить Alpha Mask и Alpha Blend.
9. Проверить Double-Sided.
10. Проверить Viewport и Asset Preview.
11. Переимпортировать модель и текстуру.
12. Сохранить и перезагрузить сцену.

## PBR verification (Phase 6.3)

- CameraUniform layout совпадает со WGSL (view_proj + camera_pos).
- Vertex layout: position, color, normal, uv, tangent.
- PbrMaterialUniform: base color, emissive, alpha cutoff, metallic, roughness, texture flags, alpha mode, double-sided.
- 11-binding material bind group layout.
- upload_mesh сохраняет uv/tangent.
- upload_texture: mip chain, sRGB/linear views, sampler settings.
- Material bind group caching + invalidation по ревизии текстуры.
- Pipeline variants: double-sided, alpha mask, alpha blend, MSAA, wireframe.
- Offscreen PBR рендеринг, pixel output.

## Performance benchmark (Phase 7/8)

Сценарии: 100 / 1000 / 10000 объектов. Распределение: все видимы, большинство вне фрустума, вокруг камеры. Метрики: candidate renderables, visible, culled, culling time, render prep time, CPU/GPU frame time, draw calls. Измерять до и после.

## Ограничения

- GPU-тесты, ручные проверки редактора, визуальные проверки не считаются выполненными, пока не проведены фактически.
- Если невозможно — указать причину.
- Текущий HEAD: f97b5ae (Phase 8.1 Frustum Culling). Shadow pass отключён. PBR не интегрирован в production.
