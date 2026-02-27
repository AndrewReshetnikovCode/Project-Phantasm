use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use phantasm_core::World;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    method: String,
    params: Option<Value>,
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    id: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Clone)]
pub enum ScriptCommand {
    LoadScript { source: String },
}

pub struct DevSession {
    pub world: Mutex<World>,
    pub script_commands: Mutex<Vec<ScriptCommand>>,
    pub project_path: PathBuf,
    undo_stack: Mutex<Vec<Value>>,
    redo_stack: Mutex<Vec<Value>>,
}

impl DevSession {
    pub fn new(world: World, project_path: PathBuf) -> Self {
        Self {
            world: Mutex::new(world),
            script_commands: Mutex::new(Vec::new()),
            project_path,
            undo_stack: Mutex::new(Vec::new()),
            redo_stack: Mutex::new(Vec::new()),
        }
    }

    fn push_undo(&self) {
        let w = self.world.lock().unwrap();
        let snapshot = w.snapshot();
        drop(w);
        let mut undo = self.undo_stack.lock().unwrap();
        undo.push(snapshot);
        if undo.len() > 50 {
            undo.remove(0);
        }
        self.redo_stack.lock().unwrap().clear();
    }
}

pub struct AgentServer;

impl AgentServer {
    pub async fn start(addr: &str, session: Arc<DevSession>) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        log::info!("Agent JSON-RPC server listening on {}", addr);

        loop {
            let (stream, peer) = listener.accept().await?;
            log::info!("Agent connected from {}", peer);
            let session = session.clone();

            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut lines = BufReader::new(reader).lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    let response = handle_request(&line, &session);
                    let response_json = serde_json::to_string(&response).unwrap_or_default();
                    let _ = writer.write_all(response_json.as_bytes()).await;
                    let _ = writer.write_all(b"\n").await;
                    let _ = writer.flush().await;
                }

                log::info!("Agent disconnected from {}", peer);
            });
        }
    }
}

fn handle_request(json_str: &str, session: &Arc<DevSession>) -> JsonRpcResponse {
    let request: JsonRpcRequest = match serde_json::from_str(json_str) {
        Ok(r) => r,
        Err(e) => {
            return error_response(Value::Null, -32700, &format!("Parse error: {}", e));
        }
    };

    let id = request.id.clone().unwrap_or(Value::Null);
    let params = request.params.unwrap_or(Value::Null);

    let result = match request.method.as_str() {
        // ── Entity tools (scene editing) ──
        "entity.spawn" => entity_spawn(session, &params),
        "entity.despawn" => entity_despawn(session, &params),
        "entity.list" => entity_list(session),
        "entity.get" => entity_get(session, &params),
        "entity.set" => entity_set(session, &params),
        "entity.find" => entity_find(session, &params),
        "entity.duplicate" => entity_duplicate(session, &params),
        "entity.remove_component" => entity_remove_component(session, &params),

        // ── World / snapshot ──
        "world.snapshot" => world_snapshot(session),
        "world.load_snapshot" => world_load_snapshot(session, &params),
        "world.undo" => world_undo(session),
        "world.redo" => world_redo(session),

        // ── Component schema ──
        "component.list_types" => component_list_types(session),
        "component.schema" => component_schema(session, &params),
        "component.register" => component_register(session, &params),

        // ── Scene file management (save/load to disk) ──
        "scene.save" => scene_save(session, &params),
        "scene.load" => scene_load(session, &params),
        "scene.list" => scene_list(session),

        // ── Script file management ──
        "script.run" => script_run(session, &params),
        "script.create" => script_create(session, &params),
        "script.edit" => script_edit(session, &params),
        "script.read" => script_read(session, &params),
        "script.list" => script_list(session),
        "script.delete" => script_delete(session, &params),

        // ── Data files (loot tables, configs, etc.) ──
        "data.create" => data_create(session, &params),
        "data.read" => data_read(session, &params),
        "data.update" => data_update(session, &params),
        "data.list" => data_list(session),
        "data.delete" => data_delete(session, &params),

        // ── Rendering / inspection ──
        "render.text_capture" => render_text_capture(session),

        // ── Project info ──
        "project.info" => project_info(session),

        _ => Err((-32601, format!("Method not found: {}", request.method))),
    };

    match result {
        Ok(value) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            result: Some(value),
            error: None,
            id,
        },
        Err((code, message)) => error_response(id, code, &message),
    }
}

fn error_response(id: Value, code: i32, message: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_string(),
        }),
        id,
    }
}

type R = Result<Value, (i32, String)>;

fn lock_err<T>(e: T) -> (i32, String)
where
    T: std::fmt::Display,
{
    (-32000, e.to_string())
}

// ═══════════════════════════════════════════════════════════════
//  Entity tools — the AI agent uses these to build scenes
// ═══════════════════════════════════════════════════════════════

fn entity_spawn(session: &Arc<DevSession>, params: &Value) -> R {
    session.push_undo();
    let mut w = session.world.lock().map_err(lock_err)?;
    let entity = w.spawn();

    if let Some(components) = params.get("components").and_then(|v| v.as_object()) {
        for (name, data) in components {
            w.insert(entity, name, data.clone()).map_err(lock_err)?;
        }
    }
    Ok(json!({"entity_id": entity}))
}

fn entity_despawn(session: &Arc<DevSession>, params: &Value) -> R {
    session.push_undo();
    let mut w = session.world.lock().map_err(lock_err)?;
    let entity_id = params
        .get("entity_id")
        .and_then(|v| v.as_u64())
        .ok_or((-32602, "Missing entity_id".into()))?;
    let removed = w.despawn(entity_id);
    Ok(json!({"removed": removed}))
}

fn entity_list(session: &Arc<DevSession>) -> R {
    let w = session.world.lock().map_err(lock_err)?;
    let mut entities: Vec<Value> = Vec::new();
    let mut sorted: Vec<u64> = w.alive.iter().copied().collect();
    sorted.sort();
    for entity_id in sorted {
        let comps = w.entity_components(entity_id);
        let name = comps
            .get("Name")
            .and_then(|v| v.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let comp_names: Vec<&str> = comps.keys().map(|s| s.as_str()).collect();
        entities.push(json!({
            "entity_id": entity_id,
            "name": name,
            "components": comp_names,
        }));
    }
    Ok(json!({"entities": entities, "count": entities.len()}))
}

fn entity_get(session: &Arc<DevSession>, params: &Value) -> R {
    let w = session.world.lock().map_err(lock_err)?;
    let entity_id = params
        .get("entity_id")
        .and_then(|v| v.as_u64())
        .ok_or((-32602, "Missing entity_id".into()))?;
    if !w.alive.contains(&entity_id) {
        return Err((-32000, format!("Entity {} not found", entity_id)));
    }
    let components = w.entity_components(entity_id);
    Ok(json!({"entity_id": entity_id, "components": components}))
}

fn entity_set(session: &Arc<DevSession>, params: &Value) -> R {
    session.push_undo();
    let mut w = session.world.lock().map_err(lock_err)?;
    let entity_id = params
        .get("entity_id")
        .and_then(|v| v.as_u64())
        .ok_or((-32602, "Missing entity_id".into()))?;

    if let Some(components) = params.get("components").and_then(|v| v.as_object()) {
        for (name, data) in components {
            w.insert(entity_id, name, data.clone()).map_err(lock_err)?;
        }
    }
    Ok(json!({"ok": true}))
}

fn entity_find(session: &Arc<DevSession>, params: &Value) -> R {
    let w = session.world.lock().map_err(lock_err)?;

    let mut results = Vec::new();

    if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
        for eid in w.find_by_name(name) {
            let comps = w.entity_components(eid);
            results.push(json!({"entity_id": eid, "components": comps}));
        }
    } else if let Some(component) = params.get("component").and_then(|v| v.as_str()) {
        for eid in w.find_by_component(component) {
            let comps = w.entity_components(eid);
            results.push(json!({"entity_id": eid, "components": comps}));
        }
    } else {
        return Err((-32602, "Provide 'name' or 'component' to search".into()));
    }

    Ok(json!({"results": results, "count": results.len()}))
}

fn entity_duplicate(session: &Arc<DevSession>, params: &Value) -> R {
    session.push_undo();
    let mut w = session.world.lock().map_err(lock_err)?;
    let entity_id = params
        .get("entity_id")
        .and_then(|v| v.as_u64())
        .ok_or((-32602, "Missing entity_id".into()))?;
    let new_id = w
        .duplicate(entity_id)
        .ok_or((-32000, format!("Entity {} not found", entity_id)))?;
    Ok(json!({"original": entity_id, "duplicate": new_id}))
}

fn entity_remove_component(session: &Arc<DevSession>, params: &Value) -> R {
    session.push_undo();
    let mut w = session.world.lock().map_err(lock_err)?;
    let entity_id = params
        .get("entity_id")
        .and_then(|v| v.as_u64())
        .ok_or((-32602, "Missing entity_id".into()))?;
    let component = params
        .get("component")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing component name".into()))?;
    let removed = w.remove(entity_id, component);
    Ok(json!({"removed": removed.is_some()}))
}

// ═══════════════════════════════════════════════════════════════
//  World / undo-redo
// ═══════════════════════════════════════════════════════════════

fn world_snapshot(session: &Arc<DevSession>) -> R {
    let w = session.world.lock().map_err(lock_err)?;
    Ok(w.snapshot())
}

fn world_load_snapshot(session: &Arc<DevSession>, params: &Value) -> R {
    session.push_undo();
    let mut w = session.world.lock().map_err(lock_err)?;
    w.load_snapshot(params).map_err(lock_err)?;
    Ok(json!({"ok": true}))
}

fn world_undo(session: &Arc<DevSession>) -> R {
    let snapshot = {
        let mut undo = session.undo_stack.lock().map_err(lock_err)?;
        undo.pop()
    };
    match snapshot {
        Some(prev) => {
            let mut w = session.world.lock().map_err(lock_err)?;
            let current = w.snapshot();
            session.redo_stack.lock().map_err(lock_err)?.push(current);
            w.load_snapshot(&prev).map_err(lock_err)?;
            Ok(json!({"ok": true, "message": "Undo successful"}))
        }
        None => Ok(json!({"ok": false, "message": "Nothing to undo"})),
    }
}

fn world_redo(session: &Arc<DevSession>) -> R {
    let snapshot = {
        let mut redo = session.redo_stack.lock().map_err(lock_err)?;
        redo.pop()
    };
    match snapshot {
        Some(next) => {
            let mut w = session.world.lock().map_err(lock_err)?;
            let current = w.snapshot();
            session.undo_stack.lock().map_err(lock_err)?.push(current);
            w.load_snapshot(&next).map_err(lock_err)?;
            Ok(json!({"ok": true, "message": "Redo successful"}))
        }
        None => Ok(json!({"ok": false, "message": "Nothing to redo"})),
    }
}

// ═══════════════════════════════════════════════════════════════
//  Component schema
// ═══════════════════════════════════════════════════════════════

fn component_list_types(session: &Arc<DevSession>) -> R {
    let w = session.world.lock().map_err(lock_err)?;
    let mut types: Vec<&str> = w.schemas.keys().map(|s| s.as_str()).collect();
    types.sort();
    Ok(json!({"types": types}))
}

fn component_schema(session: &Arc<DevSession>, params: &Value) -> R {
    let w = session.world.lock().map_err(lock_err)?;
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing name".into()))?;
    let schema = w
        .schemas
        .get(name)
        .ok_or((-32000, format!("Schema '{}' not found", name)))?;
    Ok(serde_json::to_value(schema).unwrap_or(Value::Null))
}

fn component_register(session: &Arc<DevSession>, params: &Value) -> R {
    let mut w = session.world.lock().map_err(lock_err)?;
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing name".into()))?;
    let schema = phantasm_core::ComponentSchema {
        name: name.to_string(),
        fields: std::collections::BTreeMap::new(),
    };
    w.register_schema(schema);
    Ok(json!({"ok": true, "message": format!("Registered component type '{}'", name)}))
}

// ═══════════════════════════════════════════════════════════════
//  Scene file management — save work to disk, load from disk
// ═══════════════════════════════════════════════════════════════

fn scene_save(session: &Arc<DevSession>, params: &Value) -> R {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing filename (e.g. 'level1.json')".into()))?;

    let scenes_dir = session.project_path.join("scenes");
    std::fs::create_dir_all(&scenes_dir).map_err(lock_err)?;
    let path = scenes_dir.join(sanitize_filename(filename));

    let w = session.world.lock().map_err(lock_err)?;
    let scene_data = w.export_scene();
    let json_str = serde_json::to_string_pretty(&scene_data).map_err(lock_err)?;
    std::fs::write(&path, json_str).map_err(lock_err)?;

    log::info!("Scene saved to {}", path.display());
    Ok(json!({
        "ok": true,
        "path": path.display().to_string(),
        "entity_count": w.entity_count(),
    }))
}

fn scene_load(session: &Arc<DevSession>, params: &Value) -> R {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing filename".into()))?;

    let path = session
        .project_path
        .join("scenes")
        .join(sanitize_filename(filename));

    if !path.exists() {
        return Err((-32000, format!("Scene file not found: {}", path.display())));
    }

    session.push_undo();
    let scene_json = std::fs::read_to_string(&path).map_err(lock_err)?;
    let mut w = session.world.lock().map_err(lock_err)?;

    w.alive.clear();
    for storage in w.storage.values_mut() {
        storage.clear();
    }
    w.load_scene(&scene_json).map_err(lock_err)?;

    log::info!("Scene loaded from {}", path.display());
    Ok(json!({
        "ok": true,
        "path": path.display().to_string(),
        "entity_count": w.entity_count(),
    }))
}

fn scene_list(session: &Arc<DevSession>) -> R {
    let scenes_dir = session.project_path.join("scenes");
    let mut scenes = Vec::new();
    if scenes_dir.exists() {
        for entry in std::fs::read_dir(&scenes_dir).map_err(lock_err)? {
            let entry = entry.map_err(lock_err)?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json" || e == "toml") {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                scenes.push(json!({"filename": name, "size_bytes": size}));
            }
        }
    }
    Ok(json!({"scenes": scenes}))
}

// ═══════════════════════════════════════════════════════════════
//  Script file management — create / edit / read Luau files
// ═══════════════════════════════════════════════════════════════

fn script_run(session: &Arc<DevSession>, params: &Value) -> R {
    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing source".into()))?;
    let mut cmds = session.script_commands.lock().map_err(lock_err)?;
    cmds.push(ScriptCommand::LoadScript {
        source: source.to_string(),
    });
    Ok(json!({"ok": true, "message": "Script queued for execution"}))
}

fn script_create(session: &Arc<DevSession>, params: &Value) -> R {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing filename (e.g. 'enemy_ai.luau')".into()))?;
    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing source code".into()))?;

    let scripts_dir = session.project_path.join("scripts");
    std::fs::create_dir_all(&scripts_dir).map_err(lock_err)?;
    let path = scripts_dir.join(sanitize_filename(filename));

    if path.exists() {
        return Err((
            -32000,
            format!(
                "Script '{}' already exists, use script.edit to modify",
                filename
            ),
        ));
    }

    std::fs::write(&path, source).map_err(lock_err)?;
    log::info!("Script created: {}", path.display());
    Ok(json!({
        "ok": true,
        "path": path.display().to_string(),
        "message": format!("Created {}", filename),
    }))
}

fn script_edit(session: &Arc<DevSession>, params: &Value) -> R {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing filename".into()))?;
    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing source code".into()))?;

    let path = session
        .project_path
        .join("scripts")
        .join(sanitize_filename(filename));

    std::fs::write(&path, source).map_err(lock_err)?;
    log::info!("Script edited: {}", path.display());
    Ok(json!({
        "ok": true,
        "path": path.display().to_string(),
    }))
}

fn script_read(session: &Arc<DevSession>, params: &Value) -> R {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing filename".into()))?;

    let path = session
        .project_path
        .join("scripts")
        .join(sanitize_filename(filename));

    if !path.exists() {
        return Err((-32000, format!("Script not found: {}", filename)));
    }

    let source = std::fs::read_to_string(&path).map_err(lock_err)?;
    Ok(json!({"filename": filename, "source": source}))
}

fn script_list(session: &Arc<DevSession>) -> R {
    let scripts_dir = session.project_path.join("scripts");
    let mut scripts = Vec::new();
    if scripts_dir.exists() {
        for entry in std::fs::read_dir(&scripts_dir).map_err(lock_err)? {
            let entry = entry.map_err(lock_err)?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "luau" || e == "lua") {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                scripts.push(json!({"filename": name.to_string(), "size_bytes": size}));
            }
        }
    }
    Ok(json!({"scripts": scripts}))
}

fn script_delete(session: &Arc<DevSession>, params: &Value) -> R {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing filename".into()))?;

    let path = session
        .project_path
        .join("scripts")
        .join(sanitize_filename(filename));

    if !path.exists() {
        return Err((-32000, format!("Script not found: {}", filename)));
    }
    std::fs::remove_file(&path).map_err(lock_err)?;
    log::info!("Script deleted: {}", path.display());
    Ok(json!({"ok": true, "deleted": filename}))
}

// ═══════════════════════════════════════════════════════════════
//  Data files — loot tables, enemy configs, dialogue trees, etc.
//  Stored as JSON in project/data/
// ═══════════════════════════════════════════════════════════════

fn data_create(session: &Arc<DevSession>, params: &Value) -> R {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing filename (e.g. 'loot_table.json')".into()))?;
    let data = params
        .get("data")
        .ok_or((-32602, "Missing data object".into()))?;

    let data_dir = session.project_path.join("data");
    std::fs::create_dir_all(&data_dir).map_err(lock_err)?;
    let path = data_dir.join(sanitize_filename(filename));

    let json_str = serde_json::to_string_pretty(data).map_err(lock_err)?;
    std::fs::write(&path, json_str).map_err(lock_err)?;

    log::info!("Data file created: {}", path.display());
    Ok(json!({"ok": true, "path": path.display().to_string()}))
}

fn data_read(session: &Arc<DevSession>, params: &Value) -> R {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing filename".into()))?;

    let path = session
        .project_path
        .join("data")
        .join(sanitize_filename(filename));

    if !path.exists() {
        return Err((-32000, format!("Data file not found: {}", filename)));
    }

    let content = std::fs::read_to_string(&path).map_err(lock_err)?;
    let data: Value = serde_json::from_str(&content).unwrap_or_else(|_| json!({"_raw": content}));
    Ok(json!({"filename": filename, "data": data}))
}

fn data_update(session: &Arc<DevSession>, params: &Value) -> R {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing filename".into()))?;
    let data = params
        .get("data")
        .ok_or((-32602, "Missing data object".into()))?;

    let path = session
        .project_path
        .join("data")
        .join(sanitize_filename(filename));

    let json_str = serde_json::to_string_pretty(data).map_err(lock_err)?;
    std::fs::write(&path, json_str).map_err(lock_err)?;

    log::info!("Data file updated: {}", path.display());
    Ok(json!({"ok": true, "path": path.display().to_string()}))
}

fn data_list(session: &Arc<DevSession>) -> R {
    let data_dir = session.project_path.join("data");
    let mut files = Vec::new();
    if data_dir.exists() {
        for entry in std::fs::read_dir(&data_dir).map_err(lock_err)? {
            let entry = entry.map_err(lock_err)?;
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                files.push(json!({"filename": name.to_string(), "size_bytes": size}));
            }
        }
    }
    Ok(json!({"files": files}))
}

fn data_delete(session: &Arc<DevSession>, params: &Value) -> R {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing filename".into()))?;

    let path = session
        .project_path
        .join("data")
        .join(sanitize_filename(filename));

    if !path.exists() {
        return Err((-32000, format!("Data file not found: {}", filename)));
    }
    std::fs::remove_file(&path).map_err(lock_err)?;
    Ok(json!({"ok": true, "deleted": filename}))
}

// ═══════════════════════════════════════════════════════════════
//  Render / project info
// ═══════════════════════════════════════════════════════════════

fn render_text_capture(session: &Arc<DevSession>) -> R {
    let w = session.world.lock().map_err(lock_err)?;
    let text = w.text_capture();
    Ok(json!({"text": text}))
}

fn project_info(session: &Arc<DevSession>) -> R {
    let w = session.world.lock().map_err(lock_err)?;

    let config_path = session.project_path.join("project.toml");
    let config_str = std::fs::read_to_string(&config_path).unwrap_or_default();
    let config: Value = toml::from_str::<toml::Value>(&config_str)
        .map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);

    let undo_len = session.undo_stack.lock().map(|u| u.len()).unwrap_or(0);
    let redo_len = session.redo_stack.lock().map(|r| r.len()).unwrap_or(0);

    Ok(json!({
        "project_path": session.project_path.display().to_string(),
        "config": config,
        "entity_count": w.entity_count(),
        "component_types": w.schemas.len(),
        "frame": w.frame,
        "undo_available": undo_len,
        "redo_available": redo_len,
    }))
}

fn sanitize_filename(name: &str) -> String {
    name.replace("..", "")
        .replace(['/', '\\'], "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_session() -> Arc<DevSession> {
        let dir = std::env::temp_dir().join("phantasm_test");
        let _ = std::fs::create_dir_all(dir.join("scenes"));
        let _ = std::fs::create_dir_all(dir.join("scripts"));
        let _ = std::fs::create_dir_all(dir.join("data"));
        Arc::new(DevSession::new(World::new(), dir))
    }

    #[test]
    fn spawn_and_find_by_name() {
        let session = test_session();
        let req = r#"{"jsonrpc":"2.0","method":"entity.spawn","params":{"components":{"Position":{"x":5,"y":10},"Name":{"name":"Gold Coin"}}},"id":1}"#;
        let resp = handle_request(req, &session);
        assert!(resp.error.is_none());

        let req = r#"{"jsonrpc":"2.0","method":"entity.find","params":{"name":"gold"},"id":2}"#;
        let resp = handle_request(req, &session);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["count"], 1);
    }

    #[test]
    fn duplicate_entity_via_rpc() {
        let session = test_session();
        let req = r#"{"jsonrpc":"2.0","method":"entity.spawn","params":{"components":{"Position":{"x":1,"y":2}}},"id":1}"#;
        let resp = handle_request(req, &session);
        let eid = resp.result.unwrap()["entity_id"].as_u64().unwrap();

        let req = format!(
            r#"{{"jsonrpc":"2.0","method":"entity.duplicate","params":{{"entity_id":{}}},"id":2}}"#,
            eid
        );
        let resp = handle_request(&req, &session);
        assert!(resp.error.is_none());
        let dup_id = resp.result.unwrap()["duplicate"].as_u64().unwrap();
        assert_ne!(eid, dup_id);
    }

    #[test]
    fn undo_redo_cycle() {
        let session = test_session();

        // Spawn entity
        let req = r#"{"jsonrpc":"2.0","method":"entity.spawn","params":{"components":{"Name":{"name":"Temp"}}},"id":1}"#;
        handle_request(req, &session);
        assert_eq!(session.world.lock().unwrap().entity_count(), 1);

        // Undo → entity gone
        let req = r#"{"jsonrpc":"2.0","method":"world.undo","id":2}"#;
        let resp = handle_request(req, &session);
        assert!(resp.result.unwrap()["ok"].as_bool().unwrap());
        assert_eq!(session.world.lock().unwrap().entity_count(), 0);

        // Redo → entity back
        let req = r#"{"jsonrpc":"2.0","method":"world.redo","id":3}"#;
        handle_request(req, &session);
        assert_eq!(session.world.lock().unwrap().entity_count(), 1);
    }

    #[test]
    fn scene_save_and_load() {
        let session = test_session();

        // Spawn entity
        let req = r#"{"jsonrpc":"2.0","method":"entity.spawn","params":{"components":{"Position":{"x":5,"y":10},"Name":{"name":"Saved"}}},"id":1}"#;
        handle_request(req, &session);

        // Save scene
        let req = r#"{"jsonrpc":"2.0","method":"scene.save","params":{"filename":"test_scene.json"},"id":2}"#;
        let resp = handle_request(req, &session);
        assert!(resp.error.is_none());

        // Load scene (clears and reloads)
        let req = r#"{"jsonrpc":"2.0","method":"scene.load","params":{"filename":"test_scene.json"},"id":3}"#;
        let resp = handle_request(req, &session);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["entity_count"], 1);
    }

    #[test]
    fn data_file_crud() {
        let session = test_session();

        // Create loot table
        let req = r#"{"jsonrpc":"2.0","method":"data.create","params":{"filename":"loot_table.json","data":{"goblin":{"gold":[1,5],"items":["sword","shield"]}}},"id":1}"#;
        let resp = handle_request(req, &session);
        assert!(resp.error.is_none());

        // Read it back
        let req = r#"{"jsonrpc":"2.0","method":"data.read","params":{"filename":"loot_table.json"},"id":2}"#;
        let resp = handle_request(req, &session);
        assert!(resp.error.is_none());
        let data = &resp.result.unwrap()["data"];
        assert!(data["goblin"]["items"].as_array().unwrap().len() == 2);

        // List data files
        let req = r#"{"jsonrpc":"2.0","method":"data.list","id":3}"#;
        let resp = handle_request(req, &session);
        assert_eq!(resp.result.unwrap()["files"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn project_info_returns_stats() {
        let session = test_session();
        let req = r#"{"jsonrpc":"2.0","method":"project.info","id":1}"#;
        let resp = handle_request(req, &session);
        assert!(resp.error.is_none());
        let info = resp.result.unwrap();
        assert!(info["entity_count"].as_u64().is_some());
        assert!(info["component_types"].as_u64().is_some());
    }

    #[test]
    fn unknown_method_returns_error() {
        let session = test_session();
        let json = r#"{"jsonrpc":"2.0","method":"nonexistent","id":1}"#;
        let resp = handle_request(json, &session);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }
}
