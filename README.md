# Rust Engine Editor

Native Rust 3D engine and editor UI built with `winit`, `wgpu`, and `egui`.
The editor is integrated into the existing `winit`/`wgpu` renderer (rather than
using `eframe`, which would introduce a second window/rendering lifecycle).

## Requirements

- Rust stable toolchain with Cargo
- On Windows, the graphics drivers and system build prerequisites required by `wgpu`

## Run

```powershell
cargo run
```

## Build release

```powershell
cargo build --release
```

The executable is written to `target\release\engine.exe`.

## Editor controls and features

- **Docking & Tabs**: Drag panel tabs to rearrange, split panels horizontally or vertically, group into tabs, or right-click a tab to detach into a floating window.
- **Workspaces**: Switch presets via Toolbar or **Window → Workspaces**: *Default*, *Level Editing*, *Asset Editing*, *Scripting*, *Minimal*.
- **World Outliner**: Hierarchical scene actor tree with search, filtering, visibility toggles, multi-select, and context menus.
- **Details**: Full Transform controls (Location, Rotation, Scale), components, and properties for selected entities.
- **Content Browser**: Dual-mode (List and Grid view) asset browser with breadcrumbs, type filtering, and search.
- **Viewport**: Interactive 3D camera controls (Orbit, Pan, Zoom, Focus 'F'), coordinate ground grid, and transform gizmos.
- **Output Log**: Filterable runtime log with level badges (INFO, WARN, ERROR).
- **Script Editor**: In-editor code editor for runtime controllers and WGSL shaders with hot-reload support.
- **Graphics Settings**: Real-time MSAA variants (1x, 4x, 8x), V-Sync, pipeline wireframe toggle, and camera settings.
- **Hotkeys**:
  - `Ctrl+S` — Save active scene
  - `Ctrl+O` — Load scene
  - `Ctrl+R` — Reset layout to default
  - `F1`–`F7` — Quick-focus editor panels
  - `F` — Focus camera on selected actor
  - `Delete` — Delete selected actor(s)
  - `Ctrl+D` — Duplicate selected actor(s)

The Build and Play commands are explicitly marked as unavailable until those engine subsystems exist; they are not presented as working features.

