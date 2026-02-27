use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use phantasm_agent::{AgentServer, DevSession, ScriptCommand};
use phantasm_core::World;
use phantasm_input::InputSystem;
use phantasm_render::ConsoleRenderer;
use phantasm_script::ScriptEngine;

fn lua_err(e: mlua::prelude::LuaError) -> anyhow::Error {
    anyhow::anyhow!("{}", e)
}

#[derive(Parser)]
#[command(name = "phantasm", about = "Phantasm - AI-Agent-Native Game Engine")]
struct Args {
    /// Path to the game project directory
    #[arg(long, default_value = "games/hello")]
    project: String,

    /// Run in headless mode (no terminal UI, agent-only)
    #[arg(long)]
    headless: bool,

    /// Port for the JSON-RPC agent server
    #[arg(long, default_value_t = 9000)]
    port: u16,
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();

    log::info!("Phantasm Engine v0.2.0 starting...");
    log::info!("Project: {}", args.project);
    log::info!(
        "Mode: {}",
        if args.headless {
            "headless (development)"
        } else {
            "interactive"
        }
    );

    let project_path = PathBuf::from(&args.project);
    let config_path = project_path.join("project.toml");
    let config_str = std::fs::read_to_string(&config_path).unwrap_or_else(|_| {
        log::warn!(
            "No project.toml found at {}, using defaults",
            config_path.display()
        );
        String::new()
    });
    let config: toml::Value = config_str
        .parse()
        .unwrap_or(toml::Value::Table(toml::map::Map::new()));

    let mut world = World::new();

    let scene_file = config
        .get("scenes")
        .and_then(|s| s.get("default"))
        .and_then(|s| s.as_str())
        .unwrap_or("scenes/main.json");

    let scene_path = project_path.join(scene_file);
    if scene_path.exists() {
        let scene_json = std::fs::read_to_string(&scene_path)?;
        world.load_scene(&scene_json)?;
        log::info!("Loaded scene: {}", scene_file);
    } else {
        log::warn!("Scene file not found: {}", scene_path.display());
    }

    let _audio = phantasm_audio::AudioEngine::new();

    let mut script_engine = ScriptEngine::new().map_err(lua_err)?;

    let scripts_dir = project_path.join("scripts");
    if scripts_dir.exists() {
        let mut entries: Vec<_> = std::fs::read_dir(&scripts_dir)?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|ext| ext == "luau" || ext == "lua")
            {
                let source = std::fs::read_to_string(&path)?;
                script_engine.load_script(&source).map_err(lua_err)?;
                log::info!("Loaded script: {}", path.display());
            }
        }
    }

    script_engine.call_init(&mut world)?;
    for msg in script_engine.drain_logs() {
        log::info!("[Luau] {}", msg);
    }

    log::info!(
        "World initialized: {} entities, {} component types",
        world.entity_count(),
        world.schemas.len()
    );

    let session = Arc::new(DevSession::new(world, project_path));

    let agent_addr = format!("0.0.0.0:{}", args.port);
    let session_for_agent = session.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            if let Err(e) = AgentServer::start(&agent_addr, session_for_agent).await {
                log::error!("Agent server error: {}", e);
            }
        });
    });

    if args.headless {
        run_headless(session, &mut script_engine, args.port)
    } else {
        run_interactive(session, &mut script_engine, args.port)
    }
}

fn run_headless(
    session: Arc<DevSession>,
    script_engine: &mut ScriptEngine,
    port: u16,
) -> anyhow::Result<()> {
    log::info!(
        "Headless development mode. Agent server on port {}. Ctrl+C to exit.",
        port
    );

    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    unsafe {
        libc_signal(2, move || {
            r.store(false, std::sync::atomic::Ordering::Relaxed);
        });
    }

    while running.load(std::sync::atomic::Ordering::Relaxed) {
        process_script_commands(&session, script_engine);

        for msg in script_engine.drain_logs() {
            log::info!("[Luau] {}", msg);
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    log::info!("Shutting down...");
    let w = session.world.lock().unwrap();
    log::info!(
        "Final state: {} entities, frame {}",
        w.entity_count(),
        w.frame
    );
    Ok(())
}

fn run_interactive(
    session: Arc<DevSession>,
    script_engine: &mut ScriptEngine,
    port: u16,
) -> anyhow::Result<()> {
    let mut renderer = ConsoleRenderer::new()?;
    let mut input = InputSystem::new();
    let frame_duration = Duration::from_secs_f64(1.0 / 30.0);

    renderer.add_message(format!(
        "Phantasm Engine started. Agent server on port {}",
        port
    ));

    loop {
        let frame_start = Instant::now();

        process_script_commands(&session, script_engine);

        let should_quit = input.poll(0);
        if should_quit {
            break;
        }

        let pressed = input.pressed_actions();
        let dt = frame_duration.as_secs_f64();

        {
            let mut w = session.world.lock().unwrap();

            if let Err(e) = script_engine.call_update(&mut w, dt, &pressed) {
                renderer.add_message(format!("Script error: {}", e));
            }

            for msg in script_engine.drain_logs() {
                renderer.add_message(msg);
            }

            w.frame += 1;
            renderer.render_world(&w);
        }

        renderer.flush()?;

        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }

    drop(renderer);
    log::info!("Phantasm Engine shut down gracefully.");
    Ok(())
}

fn process_script_commands(session: &Arc<DevSession>, script_engine: &mut ScriptEngine) {
    let pending: Vec<ScriptCommand> = {
        let mut cmds = session.script_commands.lock().unwrap();
        cmds.drain(..).collect()
    };

    for cmd in pending {
        match cmd {
            ScriptCommand::LoadScript { source } => {
                if let Err(e) = script_engine.load_script(&source) {
                    log::error!("Script load error: {}", e);
                } else {
                    let mut w = session.world.lock().unwrap();
                    if let Err(e) = script_engine.call_init(&mut w) {
                        log::error!("Script init error: {}", e);
                    }
                    for msg in script_engine.drain_logs() {
                        log::info!("[Luau] {}", msg);
                    }
                    log::info!("Script loaded and initialized via agent");
                }
            }
        }
    }
}

unsafe fn libc_signal<F: Fn() + Send + 'static>(_signum: i32, _handler: F) {
    // Minimal signal handling
}
