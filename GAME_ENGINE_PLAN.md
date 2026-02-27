# Project Phantasm — AI-Agent-Native Game Engine

> A game engine designed from the ground up for AI-agent-driven development: every subsystem is introspectable, scriptable, and describable in plain text so that an LLM-based agent can author, debug, and iterate on a game without a human touching the editor.

---

## Table of Contents

1. [Design Principles](#1-design-principles)
2. [Programming Language](#2-programming-language)
3. [Build System & Project Structure](#3-build-system--project-structure)
4. [Core Runtime / ECS Architecture](#4-core-runtime--ecs-architecture)
5. [Scene & Asset Format](#5-scene--asset-format)
6. [Rendering](#6-rendering)
7. [Physics](#7-physics)
8. [Audio](#8-audio)
9. [Input System](#9-input-system)
10. [Scripting / Gameplay Layer](#10-scripting--gameplay-layer)
11. [AI-Agent Interface](#11-ai-agent-interface)
12. [Editor & Tooling](#12-editor--tooling)
13. [Networking / Multiplayer](#13-networking--multiplayer)
14. [Platform & Distribution](#14-platform--distribution)
15. [Testing & CI](#15-testing--ci)
16. [Roadmap](#16-roadmap)

---

## 1. Design Principles

These principles distinguish Phantasm from every existing engine:

| # | Principle | Why it matters for AI agents |
|---|-----------|------------------------------|
| 1 | **Text-first** | Every piece of game state can be read/written as structured text (JSON, TOML, DSL). Agents work with tokens, not binary blobs. |
| 2 | **Deterministic by default** | Fixed-timestep, seeded RNG, reproducible replays. An agent can reason about cause → effect. |
| 3 | **Introspectable** | Full runtime reflection: list every entity, component, system, asset, and their schemas at any moment. |
| 4 | **Hot-reloadable** | Code and asset changes apply without restarting. Agents iterate in tight loops. |
| 5 | **Diffable** | Scene files, prefabs, and configs produce clean diffs so agents can use version control natively. |
| 6 | **Error-resilient** | Malformed agent output must never crash the engine. Validate, report, and recover. |
| 7 | **Observable** | Built-in metrics, event logs, and frame captures for the agent to evaluate its own output. |

---

## 2. Programming Language

The core engine language is the most consequential choice. Below are the realistic options, evaluated against AI-agent ergonomics.

### Option A — Rust

| Pros | Cons |
|------|------|
| Memory-safe without GC; ideal for a runtime that must never crash from agent-generated input | Steep learning curve; slower compile times |
| Excellent ecosystem: `wgpu`, `bevy_ecs`, `winit`, `naga` | Borrow checker friction when prototyping rapid API changes |
| `cbindgen` / `uniffi` for FFI to scripting layers | AI agents generate Rust with more errors than C or Python |
| Strong type system makes APIs self-documenting | |

### Option B — C++ (Modern, C++20/23)

| Pros | Cons |
|------|------|
| Largest existing gamedev ecosystem (Vulkan, OpenAL, PhysX all C/C++ native) | UB foot-guns; agent-generated C++ is risky to compile & run unsandboxed |
| Mature tooling (CMake, Conan, sanitizers) | Header complexity makes introspection harder to bolt on |
| AI agents have vast C++ training data | Slow iteration (compile + link) |

### Option C — Zig

| Pros | Cons |
|------|------|
| C-level performance with safer defaults; `comptime` reflection is excellent for introspection | Smaller ecosystem; fewer agent training examples |
| Seamless C interop (use any C library directly) | Language still pre-1.0; API instability risk |
| Fast compilation; single-binary output | Tooling (LSP, debugger) less mature |

### Option D — C core + Lua/Python surface

| Pros | Cons |
|------|------|
| Minimal core in C for speed; all gameplay in a language AI agents are best at | Two-language boundary adds complexity |
| Agents can safely iterate in the scripting layer only | Performance ceiling for script-heavy games |
| Proven model (Love2D, Defold, Godot-Python) | |

### Recommendation

**Rust** for the engine core, with a **Lua (or Luau)** embedded scripting layer for gameplay.

Rationale: Rust gives us safety guarantees that protect us from agent-generated chaos, strong reflection via `#[derive]` macros, and the `wgpu` ecosystem for cross-platform GPU access. Lua is lightweight, sandboxable, and AI agents produce correct Lua at a higher rate than Rust. The agent talks to the engine primarily through a text-based RPC interface (see §11), not by writing Rust.

---

## 3. Build System & Project Structure

### Approaches

| Approach | Tool | Notes |
|----------|------|-------|
| **A — Cargo workspace** | `cargo` | Native Rust; workspace of crates (`phantasm-core`, `phantasm-render`, `phantasm-physics`, …). Simple, well-understood. |
| **B — Cargo + Just/Make** | `cargo` + `just` | Add a top-level `justfile` with high-level commands (`just run`, `just test`, `just agent-serve`). More agent-friendly than raw cargo invocations. |
| **C — Nix flake** | `nix` | Fully reproducible dev environment. Excellent for CI, but steep entry barrier. |

### Recommendation

**Cargo workspace + `justfile`**. The `justfile` becomes the agent's "menu" of actions.

### Proposed Crate Layout

```
phantasm/
├── justfile
├── Cargo.toml              # workspace root
├── crates/
│   ├── phantasm-core/      # ECS, app lifecycle, scheduler
│   ├── phantasm-render/    # wgpu renderer, scene graph
│   ├── phantasm-physics/   # collision, rigid bodies
│   ├── phantasm-audio/     # spatial audio
│   ├── phantasm-input/     # unified input abstraction
│   ├── phantasm-script/    # Lua/Luau VM, hot-reload
│   ├── phantasm-net/       # networking (optional)
│   ├── phantasm-agent/     # AI-agent RPC server, tool definitions
│   └── phantasm-editor/    # headless + GUI editor
├── assets/                 # default/test assets (text formats)
├── games/                  # example game projects
│   └── hello/
│       ├── project.toml
│       ├── scenes/
│       ├── scripts/
│       └── assets/
└── docs/
```

---

## 4. Core Runtime / ECS Architecture

The Entity-Component-System pattern is the backbone. It is inherently introspectable (list components, query by archetype) and maps cleanly to text.

### Approaches

| Approach | Description | Trade-offs |
|----------|-------------|------------|
| **A — Custom archetype ECS** | Build our own sparse-set or archetype-based ECS from scratch | Full control over reflection, serialization, and agent APIs; significant engineering effort |
| **B — Wrap `bevy_ecs`** | Use Bevy's ECS as a library crate (it's modular) | Battle-tested, fast; ties us to Bevy's release cadence |
| **C — Wrap `hecs` or `shipyard`** | Lighter-weight ECS crates | Less opinionated; easier to extend; fewer built-in features |
| **D — Relational (SQLite-backed)** | Store world state in an in-memory SQLite database | Agents can literally query the world with SQL; slower for real-time; novel approach |

### Recommendation

**Option A (custom) informed by `hecs` internals**, because:

- We need deep control over reflection metadata (component schemas as JSON Schema).
- We need deterministic iteration order (reproducibility).
- We need first-class serialization of world snapshots to text.

### Key Design Decisions

- Every component type registers a **schema** (name, fields, types, defaults) at startup.
- The world can be **snapshotted** to a TOML/JSON document and **restored** from one.
- Systems declare their read/write access explicitly → the scheduler can parallelize, and the agent can understand data flow.

---

## 5. Scene & Asset Format

### Scene Format Approaches

| Approach | Format | Agent Friendliness |
|----------|--------|--------------------|
| **A — TOML scenes** | `.toml` | Very readable, clean diffs, limited nesting |
| **B — RON (Rusty Object Notation)** | `.ron` | Rust-native, good Serde support; less familiar to agents |
| **C — JSON** | `.json` | Universally understood by every LLM; verbose |
| **D — Custom DSL** | `.phantasm` | Optimized for agent generation; design overhead |

### Recommendation

**TOML for project/config files, JSON for scene data, with a thin custom DSL for scripted behaviors.**

TOML is the most diff-friendly for flat configs. JSON is what every AI model knows best for structured data. A small behavior DSL (compiled to Lua) lets agents express game logic concisely.

### Asset Pipeline

| Asset Type | Source Format | Runtime Format | Notes |
|------------|---------------|----------------|-------|
| Meshes | glTF 2.0 (`.gltf`/`.glb`) | GPU buffers | glTF is text-representable and well-documented |
| Textures | PNG / KTX2 | Compressed GPU textures | PNG for source, KTX2 for runtime |
| Audio | OGG / WAV | Decoded PCM or streaming | |
| Scenes | `.json` | In-memory ECS world | |
| Scripts | `.lua` | Lua bytecode (hot-reload from source) | |
| Shaders | WGSL (`.wgsl`) | `naga` IR → SPIR-V / MSL / HLSL | WGSL is text, agent-writable |

---

## 6. Rendering

### Approaches

| Approach | Description | Trade-offs |
|----------|-------------|------------|
| **A — `wgpu` (WebGPU API)** | Cross-platform GPU abstraction (Vulkan, Metal, DX12, WebGPU) | Modern, safe Rust API; slight overhead vs raw Vulkan; excellent portability including web |
| **B — Raw Vulkan via `ash`** | Direct Vulkan bindings | Maximum control and performance; enormous API surface; no web support |
| **C — OpenGL via `glow`** | OpenGL / OpenGL ES | Widest legacy support; dated API; no compute shaders on older targets |
| **D — Software rasterizer** | CPU rendering | Runs anywhere, trivial to debug, but very slow for 3D |

### Recommendation

**`wgpu`**. It gives us Vulkan-class features with a safe API, and we get web export for free (important for agent-driven prototyping and demos).

### Rendering Architecture

```
Frame
 ├── Extract phase    (read ECS → build render world, parallel with game logic)
 ├── Prepare phase    (sort, batch, upload GPU data)
 └── Render phase     (execute render graph)
```

- **Render graph** defined as a data structure (nodes = passes, edges = resource dependencies). The agent can add/remove/reorder passes by editing the graph description.
- **Shader hot-reload**: watch `.wgsl` files, recompile on change, report errors as structured text.
- **Frame capture**: dump any frame's render targets to PNG for agent visual inspection.

---

## 7. Physics

### Approaches

| Approach | Library / Method | Trade-offs |
|----------|-----------------|------------|
| **A — `rapier`** | Rust-native 2D/3D physics engine | Deterministic, well-maintained, Serde-serializable state; good fit |
| **B — Bullet (via FFI)** | Industry-standard C++ physics | Proven in AAA; C++ FFI complexity |
| **C — Custom minimal** | Hand-rolled AABB/SAT collision + Verlet integration | Full control; enormous effort for 3D; fine for 2D-only |
| **D — Box2D (via FFI)** | 2D only | Excellent for 2D games; no 3D path |

### Recommendation

**`rapier`** for both 2D and 3D. It is written in Rust, supports deterministic simulation (critical for agent reproducibility), and its state can be serialized/deserialized — meaning an agent can save, modify, and restore physics state as text.

---

## 8. Audio

### Approaches

| Approach | Description | Trade-offs |
|----------|-------------|------------|
| **A — `kira`** | Rust game audio library with tweening, clock sync | Feature-rich, game-oriented API |
| **B — `rodio`** | Simple Rust audio playback | Easy to integrate; fewer features |
| **C — `cpal` + custom mixer** | Low-level audio I/O; build mixer from scratch | Full control; significant work |
| **D — FMOD / Wwise (via FFI)** | Industry-standard middleware | Powerful; proprietary licenses; C FFI |

### Recommendation

**`kira`** for its game-centric API (sound groups, spatial audio, tweening). Wrap it so the agent can trigger/control audio via text commands like `play("explosion", position=[1,2,3], volume=0.8)`.

---

## 9. Input System

### Approaches

| Approach | Description | Trade-offs |
|----------|-------------|------------|
| **A — `winit` events directly** | Map OS events to engine input | Simple; tightly coupled to windowing |
| **B — Action-map abstraction** | Define named actions ("jump", "shoot") mapped to physical inputs via config | Agent-friendly: agent works with semantic actions, not keycodes |
| **C — Record/replay layer** | Log all input as timestamped events; replay for deterministic testing | Essential for agent iteration; adds storage overhead |

### Recommendation

**All three, layered**: `winit` at the bottom, action-map in the middle, record/replay on top. The agent interacts only with the action-map layer and can inject synthetic input events for testing.

### Input Config (example `input.toml`)

```toml
[actions.jump]
keyboard = ["Space"]
gamepad  = ["South"]

[actions.move]
keyboard_axis = { positive = "D", negative = "A" }
gamepad_axis  = "LeftStickX"
```

---

## 10. Scripting / Gameplay Layer

This is the primary surface the AI agent writes game logic on.

### Approaches

| Approach | Language | Trade-offs |
|----------|----------|------------|
| **A — Lua 5.4 via `mlua`** | Lua | Tiny, fast, embeddable, huge training data for LLMs; dynamic typing |
| **B — Luau via `luau` crate** | Luau (typed Lua) | Adds type annotations Lua lacks; used by Roblox; moderate ecosystem |
| **C — WASM guest modules** | Any language → WASM | Sandboxed, polyglot; startup overhead; debugging is harder |
| **D — Rhai** | Rhai (Rust-native scripting) | Safe, Rust-like syntax; small community; limited LLM training data |
| **E — Python via `pyo3`** | Python | Richest AI/ML library ecosystem; GIL and memory footprint issues |
| **F — TypeScript via `deno_core`** | TypeScript | Strong typing; large LLM training data; heavier runtime |

### Recommendation

**Luau (Option B)** as the primary scripting language.

Rationale:
- Type annotations give the agent (and the engine's validator) richer error messages.
- Luau is sandboxed by design (no `os.execute`, no file I/O) — safe for agent-generated code.
- Roblox has proven it scales to complex games.
- LLMs produce good Lua; Luau is a superset.

Expose the full ECS API to Luau:

```lua
-- Example agent-generated script
local player = world:spawn()
world:insert(player, Transform { position = vec3(0, 1, 0) })
world:insert(player, Sprite { texture = "hero.png", size = vec2(1, 2) })
world:insert(player, Health { current = 100, max = 100 })

function on_update(dt: number)
    for entity, transform, input in world:query(Transform, PlayerInput) do
        transform.position.x += input.move_x * 5 * dt
    end
end
```

### Script Hot-Reload Protocol

1. Agent writes `.lua` file via RPC.
2. Engine file-watcher detects change.
3. Engine compiles script in a **shadow VM**.
4. If compilation succeeds → swap into live VM, emit `reload_ok`.
5. If compilation fails → keep old script, emit `reload_error { line, message }`.

---

## 11. AI-Agent Interface

This is the **differentiating subsystem** of Phantasm. It is how an external AI agent (LLM running in a separate process) talks to the running engine.

### Approaches

| Approach | Protocol | Trade-offs |
|----------|----------|------------|
| **A — JSON-RPC over TCP** | JSON-RPC 2.0 | Simple, text-based, well-supported by LLM tool-use frameworks |
| **B — gRPC + Protobuf** | gRPC | Strongly typed, fast; binary format is less agent-friendly |
| **C — REST API (HTTP)** | HTTP/JSON | Universally understood; overhead per request |
| **D — stdio (MCP-style)** | JSON over stdin/stdout | Zero networking; perfect for subprocess model (like MCP servers) |
| **E — Hybrid: MCP server** | Model Context Protocol | Designed exactly for LLM ↔ tool interaction; growing ecosystem |

### Recommendation

**Option E — Implement the engine as an MCP server** (primary), with **Option A — JSON-RPC over TCP** as a secondary transport for remote/cloud agents.

MCP is purpose-built for LLM tool use. The engine exposes **tools**, **resources**, and **prompts** that any MCP-compatible client (Claude, Cursor, custom agents) can call.

### Tool Catalog (initial)

```
── World ──────────────────────────────
  world.snapshot          → returns full world state as JSON
  world.load_snapshot     ← accepts JSON, replaces world state
  world.step              → advance simulation by N frames

── Entity ─────────────────────────────
  entity.spawn            → create entity, returns ID
  entity.despawn          ← entity ID
  entity.list             → list all entities with archetypes
  entity.get              → read all components of an entity
  entity.set              ← write components (partial update)

── Component ──────────────────────────
  component.list_types    → all registered component schemas
  component.schema        → JSON Schema for a component type

── Script ─────────────────────────────
  script.create           ← path + source code
  script.edit             ← path + patch (or full replacement)
  script.list             → all loaded scripts
  script.errors           → current compilation errors

── Asset ──────────────────────────────
  asset.list              → all loaded assets with metadata
  asset.import            ← import asset from path or URL

── Render ─────────────────────────────
  render.capture_frame    → returns PNG of current frame
  render.set_camera       ← camera parameters

── Scene ──────────────────────────────
  scene.save              → serialize current scene to JSON
  scene.load              ← load scene from JSON
  scene.list              → all scene files in project

── System ─────────────────────────────
  system.list             → all registered systems, execution order
  system.toggle           ← enable/disable a system
  system.metrics          → per-system timing data

── Project ────────────────────────────
  project.info            → project metadata
  project.build           → trigger asset build pipeline
  project.run             → start game in play mode
  project.stop            → stop play mode
  project.logs            → structured log output (filterable)
```

### Observation / Feedback Loop

The agent needs feedback beyond text. Provide:

| Signal | Method | Use case |
|--------|--------|----------|
| **Frame capture** | `render.capture_frame` returns base64 PNG | Agent visually inspects the scene |
| **Structured logs** | `project.logs` with severity/category filters | Agent reads errors, warnings, gameplay events |
| **Metrics** | `system.metrics` returns frame time, entity count, etc. | Agent monitors performance |
| **Replay** | `world.snapshot` + `world.load_snapshot` + deterministic step | Agent can "rewind" and try different approaches |
| **Diff** | `scene.save` at two points → agent computes diff | Agent tracks what changed |

---

## 12. Editor & Tooling

### Approaches

| Approach | Description | Trade-offs |
|----------|-------------|------------|
| **A — Headless only (CLI + MCP)** | No GUI editor; the AI agent IS the editor | Simplest to build; alienates human users |
| **B — `egui`-based editor** | Rust immediate-mode GUI running in-engine | Fast to develop; good enough for debugging; not production-polished |
| **C — Web-based editor (served by engine)** | Engine runs an HTTP server; editor is a web app | Accessible from any browser; decoupled from engine render loop |
| **D — VS Code extension** | Custom extension that connects to engine via MCP | Leverages existing editor; familiar to developers |

### Recommendation

**Phase 1: Headless (A)**. The engine is a terminal/MCP process. The AI agent is the primary user.

**Phase 2: `egui` debug overlay (B)**. Adds a visual inspector for humans to see what the agent built.

**Phase 3: Web-based editor (C)** if the project reaches maturity. Serves a React/Svelte UI from the engine process; both human and AI agent can use it simultaneously.

---

## 13. Networking / Multiplayer

### Approaches

| Approach | Description | Trade-offs |
|----------|-------------|------------|
| **A — `laminar` / UDP custom** | Lightweight Rust UDP library | Full control; much to implement |
| **B — `quinn` (QUIC)** | Reliable + unreliable streams over QUIC | Modern protocol; built-in encryption; excellent Rust crate |
| **C — WebRTC via `webrtc-rs`** | Peer-to-peer with NAT traversal | Browser-compatible; complex |
| **D — Defer entirely** | Ship single-player first | Reduces scope dramatically |

### Recommendation

**Option D for v0.x**, then **Option B (`quinn` / QUIC)** when multiplayer is prioritized. Networking is orthogonal to the AI-agent value proposition and can be layered on later.

---

## 14. Platform & Distribution

### Target Platforms (in priority order)

| Priority | Platform | Technology |
|----------|----------|------------|
| 1 | **Linux / macOS / Windows** | Native binary via `cargo build` |
| 2 | **Web (WASM)** | `wasm-pack` + `wgpu` WebGPU backend |
| 3 | **Mobile (iOS / Android)** | Future; Rust cross-compilation |

Web is high priority because it lets an AI agent generate a game and immediately share a playable link.

---

## 15. Testing & CI

### Testing Strategy

| Layer | Tool | What it tests |
|-------|------|---------------|
| Unit | `cargo test` | Individual functions, ECS queries, serialization |
| Integration | `cargo test` + engine fixtures | Full systems interacting (e.g., physics + render extract) |
| Script | Luau test harness | Agent-generated gameplay scripts |
| Visual | Frame capture + image diff (`pixelmatch`) | Rendering correctness; regression detection |
| Agent E2E | Script that connects as an MCP client, builds a scene, captures frame | Full agent workflow |

### CI Pipeline

```
┌─────────┐    ┌──────────┐    ┌────────────┐    ┌──────────────┐
│  Lint    │───▶│  Build   │───▶│  Unit Test │───▶│ Integration  │
│ clippy + │    │ debug +  │    │            │    │  + Visual    │
│ rustfmt  │    │ release  │    │            │    │  Regression  │
└─────────┘    └──────────┘    └────────────┘    └──────────────┘
```

---

## 16. Roadmap

### Phase 0 — Foundation (Weeks 1–6)

- [ ] Cargo workspace scaffolding
- [ ] Custom ECS with reflection and JSON serialization
- [ ] Basic `wgpu` renderer (clear color + 2D sprites)
- [ ] `winit` window + input handling
- [ ] Luau scripting integration with hot-reload
- [ ] JSON-RPC / MCP server in `phantasm-agent`
- [ ] `just` commands: `run`, `test`, `agent-serve`
- [ ] First demo: agent spawns colored rectangles via MCP

### Phase 1 — Playable 2D (Weeks 7–14)

- [ ] Sprite batching, texture atlases, animation
- [ ] `rapier` 2D physics integration
- [ ] `kira` audio integration
- [ ] Scene save/load (JSON)
- [ ] Action-map input system with record/replay
- [ ] Frame capture tool for agent visual feedback
- [ ] Agent E2E test: agent builds a simple platformer

### Phase 2 — 3D & Polish (Weeks 15–24)

- [ ] 3D mesh rendering (glTF loader, PBR materials)
- [ ] 3D physics (`rapier3d`)
- [ ] Render graph with configurable passes
- [ ] `egui` debug overlay
- [ ] WASM/WebGPU export
- [ ] Shader hot-reload
- [ ] Agent E2E test: agent builds a 3D scene with lighting

### Phase 3 — Ecosystem (Weeks 25+)

- [ ] Web-based editor
- [ ] Asset store / prefab library (text-described)
- [ ] Multiplayer via QUIC
- [ ] Plugin system (Rust crate or Luau package)
- [ ] Public MCP tool registry for community extensions

---

## Appendix A — Why Not Use an Existing Engine?

| Engine | Limitation for AI-agent workflow |
|--------|----------------------------------|
| Unity | Closed-source runtime; C# scripting is heavy; binary scene format (YAML but very noisy); editor-centric workflow |
| Unreal | C++ complexity; Blueprint is visual (not text); enormous binary assets |
| Godot | Closest match (GDScript is text, scene format is text); but GDScript has limited LLM training data, and the editor is tightly coupled to the engine |
| Bevy | Strong candidate; but no built-in agent interface, no scene format stability yet, no scripting layer |

Phantasm takes the best ideas from Bevy (ECS, Rust, modularity) and Godot (text scenes, scripting) and adds a first-class AI-agent interface that none of them have.

---

## Appendix B — Example Agent Session

```
Agent → engine:  tool: component.list_types
Engine → agent:  ["Transform", "Sprite", "RigidBody2D", "Health", "PlayerInput"]

Agent → engine:  tool: entity.spawn
Engine → agent:  { "entity_id": 42 }

Agent → engine:  tool: entity.set
                 { "entity_id": 42,
                   "components": {
                     "Transform": { "position": [0, 1, 0] },
                     "Sprite": { "texture": "hero.png", "size": [1, 2] }
                   }}
Engine → agent:  { "ok": true }

Agent → engine:  tool: script.create
                 { "path": "scripts/player.lua",
                   "source": "function on_update(dt)\n  ...\nend" }
Engine → agent:  { "ok": true, "warnings": [] }

Agent → engine:  tool: render.capture_frame
Engine → agent:  { "image": "data:image/png;base64,iVBOR..." }

Agent → (vision model) → "The hero sprite is rendered at the correct position."
```
