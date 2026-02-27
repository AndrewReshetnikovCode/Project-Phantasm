# AGENTS.md

## Cursor Cloud specific instructions

### Overview

Project Phantasm is an AI-agent-native game engine for text-based/console games, written in Rust with Luau scripting. The codebase is a Cargo workspace with 7 crates.

### System Dependencies

Building requires C++ standard library headers (for Luau/mlua compilation) and ALSA dev headers (for kira/cpal audio):
```
sudo apt-get install -y g++ libstdc++-14-dev libasound2-dev
```

**Important**: Set `CXX=g++` when building, because the default `c++` (clang) on some systems cannot locate C++ standard library headers needed by the Luau build. Either export it or prefix commands: `CXX=g++ cargo build`.

### Build / Test / Lint / Run

All standard commands are in the `justfile`. Key commands:

| Action | Command |
|--------|---------|
| Build | `CXX=g++ cargo build --workspace` |
| Test | `CXX=g++ cargo test --workspace` |
| Lint | `CXX=g++ cargo clippy --workspace -- -D warnings` |
| Format check | `cargo fmt --all -- --check` |
| Run (interactive) | `CXX=g++ cargo run -p phantasm-engine -- --project games/hello` |
| Run (headless/agent) | `CXX=g++ cargo run -p phantasm-engine -- --project games/hello --headless --port 9000` |

### Architecture

- **phantasm-core**: ECS world, component schemas, JSON serialization/snapshot
- **phantasm-render**: Crossterm-based terminal renderer
- **phantasm-input**: Action-map input system with record/replay
- **phantasm-audio**: Kira audio wrapper (gracefully degrades if no audio device)
- **phantasm-script**: Luau scripting via mlua (command-buffer pattern)
- **phantasm-agent**: JSON-RPC 2.0 server over TCP for AI agent interaction
- **phantasm-engine**: Main binary tying everything together

### Agent JSON-RPC Interface

When running in headless mode (`--headless`), the engine listens for JSON-RPC connections on the specified port (default 9000). Send newline-delimited JSON-RPC 2.0 requests over TCP. Available methods: `entity.spawn`, `entity.despawn`, `entity.list`, `entity.get`, `entity.set`, `world.snapshot`, `world.load_snapshot`, `component.list_types`, `component.schema`, `script.load`, `render.text_capture`.

### Gotchas

- Audio will log a warning and run silent on headless VMs (no ALSA device). This is expected.
- The Luau `mlua::Error` type is `!Send + !Sync`, so it cannot be used with `anyhow::Error` via `?`. Use `.map_err(|e| anyhow::anyhow!("{}", e))` at boundaries.
- Interactive mode uses crossterm raw mode; make sure the terminal is restored on exit (the `Drop` impl handles this).
