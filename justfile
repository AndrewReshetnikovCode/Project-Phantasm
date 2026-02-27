# Phantasm Engine - AI-Agent-Native Game Engine

# Run the engine with the hello game (interactive terminal mode)
run:
    cargo run -p phantasm-engine -- --project games/hello

# Run the engine in headless mode (agent-only, no terminal UI)
agent port="9000":
    cargo run -p phantasm-engine -- --project games/hello --headless --port {{port}}

# Run all tests
test:
    cargo test --workspace

# Build all crates (debug)
build:
    cargo build --workspace

# Build in release mode
release:
    cargo build --workspace --release

# Run clippy lints
lint:
    cargo clippy --workspace -- -D warnings

# Check formatting
fmt-check:
    cargo fmt --all -- --check

# Format all code
fmt:
    cargo fmt --all

# Clean build artifacts
clean:
    cargo clean

# Run a specific crate's tests
test-crate crate:
    cargo test -p {{crate}}
