# AGENTS.md — Rust Engine

Persistent instructions for any coding agent working on Rust Engine.
Read this file, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, and `docs/VERIFICATION.md`
before starting any task. Never trust documentation more than the actual code.

## 1. Project context

- Name: Rust Engine.
- Language: Rust.
- Goal: universal game engine with own Editor.
- Priority: performance.
- Audience: engine developer.
- Long-term project; development happens in small, verifiable phases.
- Main technologies: Rust 2024 edition, bevy_ecs 0.16, wgpu 27, winit 0.30, egui 0.33, glam 0.30, ron 0.8, serde, gltf 1.4, image 0.25, tokio, uuid.

## 2. Mandatory reading

1. `AGENTS.md`
2. `docs/ARCHITECTURE.md`
3. `docs/ROADMAP.md`
4. `docs/VERIFICATION.md`
5. Check branch, commit SHA, `git status`.
6. Inspect relevant source.
7. Check recent commits when continuing work.

If docs are stale or contradict code, trust code and fix docs.

## 3. Architecture rules

- Do not duplicate existing systems.
- Research existing implementations before adding features.
- Do not break public interfaces without reason.
- No large refactors unless required by the task.
- Do not mix Editor, ECS, Renderer, Asset Pipeline without justification.
- GPU resources belong to renderer/resource systems, not ECS.
- ECS must not store low-level GPU objects; use handles.
- Avoid unnecessary copies, allocations, repeated GPU resource creation, extra passes.
- Do not create fake functionality to make tests pass.
- Do not implement a test-only version instead of the production version.

Concrete Rust Engine architecture:
- Bevy ECS (0.16) for components/resources/systems.
- `Renderable` component holds `Handle<MeshAsset>` + `Handle<MaterialAsset>`.
- `RenderSnapshot` collects `RenderableInstance` (entity, renderable, global transform) per frame.
- `Renderer` owns all wgpu state, asset managers, and render pipelines.
- `AssetManager<T>` generic handle-based storage; `Handle<T>` is Copy/Clone.
- `GpuResourceRegistry` tracks upload/cache/staleness.
- `sync_world_renderables` uploads meshes/materials on demand.
- Frustum culling integrated in `render_scene` and `render_scene_to_viewport`.

## 4. Standard Development Workflow

Every non-trivial task must pass these stages.

### Stage A — Audit
Before changing code:
- Check current branch, SHA, git status.
- Study existing implementation.
- Find real production code entry points.
- Trace data flow between subsystems.
- Find existing tests.
- Determine what is missing or broken.
- Ensure new implementation does not duplicate existing one.
No code changes at this stage.

### Stage B — Plan
- Concrete implementation plan.
- Split large task into independently verifiable parts.
- List affected files and subsystems.
- Define completion criteria per stage.
- List risks and integration issues.
Avoid excessive documentation and artificial splitting of trivial tasks.

### Stage C — Implementation
- Implement functionality, not scaffolding.
- Work in small completed functional increments.
- Do not touch unrelated subsystems.
- Do not create a second implementation of an existing system.
- Preserve backward compatibility when possible.
- Do not delete tests to make new ones pass.
- Do not replace production code with test implementations.

### Stage D — Verification
After implementation:
1. Run `.\scripts\verify.ps1`.
2. Fix issues.
3. Check `git diff`.
4. Check `git diff --check`.
5. Ensure functionality is actually invoked from the application runtime path.
6. Verify documentation matches implementation.
7. Explicitly list manual checks not performed.
For large changes also run:
```
.\scripts\verify.ps1 -Release
```
Do not declare task complete if acceptance criteria are unmet.

## 4b. Safe Parallel Development

- Prefer separate Git branches or worktrees for independent subsystems.
- Avoid simultaneous edits to shared files by multiple agents.
- Define responsibility boundaries before parallel work.
- Agree on public interfaces between subsystems.
- Integrate changes sequentially.
- Run full verification after merging.
- Do not parallelize tasks depending on unfinished architecture of another task.

Potential independent directions (examples only):
- Renderer and graphics.
- Editor UI.
- Asset Pipeline.
- Documentation and test infrastructure.
These are examples, not permission to change shared interfaces without coordination.

## 4. Implementation rules

- Plan before implementing complex tasks.
- Implement full functionality, not scaffolding.
- No stubs without explicit note.
- No `unwrap`/`expect` in production without justification.
- Do not suppress warnings without reason.
- Do not delete tests to make new ones pass.
- Add tests for new functionality.
- Preserve scene/asset/serialization compatibility unless task says otherwise.
- If architecture change needed, explain why and avoid technical debt.

## 5. Reality verification

Verify:
- system is used in runtime execution path.
- functions called from real path.
- UI talks to real backend.
- data flows between subsystems.
- GPU resources actually created and used.
- tests exercise production code.
- profiler numbers are real measurements.
- benchmarks do real work.
- fallbacks do not hide errors.
- settings affect behavior.

Functionality existing only in tests is NOT implemented.

## 6. Testing rules

Before finishing, run applicable:
- `cargo fmt --all -- --check`
- `cargo check`
- `cargo test`
- `cargo build --release`
- `git diff --check`

Do not claim tests passed if not run. Do not claim app ran if not run.
GPU tests, manual editor checks, visual checks must not be reported as done unless actually performed.
If impossible, state reason.

## 7. Git rules

- Check branch, working tree before work.
- Do not delete others' changes.
- No destructive git commands without need.
- No commit/push without explicit user permission unless task explicitly allows.
- Use feature branches for large tasks when possible.
- Do not mix unrelated changes in one commit.
- Report changed files and git state at end.

## 8. Documentation updates

On substantial implementation:
- Update ARCHITECTURE.md if architecture changed.
- Update ROADMAP.md if stage completed/changed/cancelled/new priorities.
- Update VERIFICATION.md if testing/launch/limits changed.
- Update AGENTS.md only if persistent rules changed.
Never leave docs contradicting code.

## 9. Final report format

### 1. Done
### 2. Changed files
### 3. Architectural decisions
### 4. Checks (command, result, test count, warnings, skipped)
### 5. Manual verification
### 6. Unfinished tasks / limitations
### 7. Git (branch, SHA before/after, status, commit/push)
### 8. Documentation updated
