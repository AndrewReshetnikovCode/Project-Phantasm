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

pub struct AgentServer;

impl AgentServer {
    pub async fn start(
        addr: &str,
        world: Arc<Mutex<World>>,
        script_commands: Arc<Mutex<Vec<ScriptCommand>>>,
    ) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        log::info!("Agent JSON-RPC server listening on {}", addr);

        loop {
            let (stream, peer) = listener.accept().await?;
            log::info!("Agent connected from {}", peer);
            let world = world.clone();
            let script_commands = script_commands.clone();

            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut lines = BufReader::new(reader).lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    let response = handle_request(&line, &world, &script_commands);
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

fn handle_request(
    json_str: &str,
    world: &Arc<Mutex<World>>,
    script_commands: &Arc<Mutex<Vec<ScriptCommand>>>,
) -> JsonRpcResponse {
    let request: JsonRpcRequest = match serde_json::from_str(json_str) {
        Ok(r) => r,
        Err(e) => {
            return error_response(Value::Null, -32700, &format!("Parse error: {}", e));
        }
    };

    let id = request.id.clone().unwrap_or(Value::Null);
    let params = request.params.unwrap_or(Value::Null);

    let result = match request.method.as_str() {
        "entity.spawn" => entity_spawn(world, &params),
        "entity.despawn" => entity_despawn(world, &params),
        "entity.list" => entity_list(world),
        "entity.get" => entity_get(world, &params),
        "entity.set" => entity_set(world, &params),
        "world.snapshot" => world_snapshot(world),
        "world.load_snapshot" => world_load_snapshot(world, &params),
        "component.list_types" => component_list_types(world),
        "component.schema" => component_schema(world, &params),
        "script.load" => script_load(script_commands, &params),
        "render.text_capture" => render_text_capture(world),
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

fn entity_spawn(world: &Arc<Mutex<World>>, params: &Value) -> Result<Value, (i32, String)> {
    let mut w = world.lock().map_err(|e| (-32000, e.to_string()))?;
    let entity = w.spawn();

    if let Some(components) = params.get("components").and_then(|v| v.as_object()) {
        for (name, data) in components {
            w.insert(entity, name, data.clone())
                .map_err(|e| (-32000, e.to_string()))?;
        }
    }

    Ok(json!({"entity_id": entity}))
}

fn entity_despawn(world: &Arc<Mutex<World>>, params: &Value) -> Result<Value, (i32, String)> {
    let mut w = world.lock().map_err(|e| (-32000, e.to_string()))?;
    let entity_id = params
        .get("entity_id")
        .and_then(|v| v.as_u64())
        .ok_or((-32602, "Missing entity_id".to_string()))?;
    let removed = w.despawn(entity_id);
    Ok(json!({"removed": removed}))
}

fn entity_list(world: &Arc<Mutex<World>>) -> Result<Value, (i32, String)> {
    let w = world.lock().map_err(|e| (-32000, e.to_string()))?;
    let mut entities: Vec<Value> = Vec::new();
    let mut sorted: Vec<u64> = w.alive.iter().copied().collect();
    sorted.sort();
    for entity_id in sorted {
        let components: Vec<String> = w.entity_components(entity_id).keys().cloned().collect();
        entities.push(json!({"entity_id": entity_id, "components": components}));
    }
    Ok(json!({"entities": entities}))
}

fn entity_get(world: &Arc<Mutex<World>>, params: &Value) -> Result<Value, (i32, String)> {
    let w = world.lock().map_err(|e| (-32000, e.to_string()))?;
    let entity_id = params
        .get("entity_id")
        .and_then(|v| v.as_u64())
        .ok_or((-32602, "Missing entity_id".to_string()))?;
    if !w.alive.contains(&entity_id) {
        return Err((-32000, format!("Entity {} not found", entity_id)));
    }
    let components = w.entity_components(entity_id);
    Ok(json!({"entity_id": entity_id, "components": components}))
}

fn entity_set(world: &Arc<Mutex<World>>, params: &Value) -> Result<Value, (i32, String)> {
    let mut w = world.lock().map_err(|e| (-32000, e.to_string()))?;
    let entity_id = params
        .get("entity_id")
        .and_then(|v| v.as_u64())
        .ok_or((-32602, "Missing entity_id".to_string()))?;

    if let Some(components) = params.get("components").and_then(|v| v.as_object()) {
        for (name, data) in components {
            w.insert(entity_id, name, data.clone())
                .map_err(|e| (-32000, e.to_string()))?;
        }
    }
    Ok(json!({"ok": true}))
}

fn world_snapshot(world: &Arc<Mutex<World>>) -> Result<Value, (i32, String)> {
    let w = world.lock().map_err(|e| (-32000, e.to_string()))?;
    Ok(w.snapshot())
}

fn world_load_snapshot(world: &Arc<Mutex<World>>, params: &Value) -> Result<Value, (i32, String)> {
    let mut w = world.lock().map_err(|e| (-32000, e.to_string()))?;
    w.load_snapshot(params)
        .map_err(|e| (-32000, e.to_string()))?;
    Ok(json!({"ok": true}))
}

fn component_list_types(world: &Arc<Mutex<World>>) -> Result<Value, (i32, String)> {
    let w = world.lock().map_err(|e| (-32000, e.to_string()))?;
    let mut types: Vec<&str> = w.schemas.keys().map(|s| s.as_str()).collect();
    types.sort();
    Ok(json!({"types": types}))
}

fn component_schema(world: &Arc<Mutex<World>>, params: &Value) -> Result<Value, (i32, String)> {
    let w = world.lock().map_err(|e| (-32000, e.to_string()))?;
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing name".to_string()))?;
    let schema = w
        .schemas
        .get(name)
        .ok_or((-32000, format!("Schema '{}' not found", name)))?;
    Ok(serde_json::to_value(schema).unwrap_or(Value::Null))
}

fn script_load(
    script_commands: &Arc<Mutex<Vec<ScriptCommand>>>,
    params: &Value,
) -> Result<Value, (i32, String)> {
    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "Missing source".to_string()))?;
    let mut cmds = script_commands
        .lock()
        .map_err(|e| (-32000, e.to_string()))?;
    cmds.push(ScriptCommand::LoadScript {
        source: source.to_string(),
    });
    Ok(json!({"ok": true, "message": "Script queued for loading"}))
}

fn render_text_capture(world: &Arc<Mutex<World>>) -> Result<Value, (i32, String)> {
    let w = world.lock().map_err(|e| (-32000, e.to_string()))?;
    let text = w.text_capture();
    Ok(json!({"text": text}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_request() {
        let json = r#"{"jsonrpc":"2.0","method":"entity.list","id":1}"#;
        let world = Arc::new(Mutex::new(World::new()));
        let cmds = Arc::new(Mutex::new(Vec::new()));
        let resp = handle_request(json, &world, &cmds);
        assert!(resp.error.is_none());
        assert!(resp.result.is_some());
    }

    #[test]
    fn spawn_via_rpc() {
        let json = r#"{"jsonrpc":"2.0","method":"entity.spawn","params":{"components":{"Position":{"x":5,"y":10},"Name":{"name":"TestEntity"}}},"id":1}"#;
        let world = Arc::new(Mutex::new(World::new()));
        let cmds = Arc::new(Mutex::new(Vec::new()));
        let resp = handle_request(json, &world, &cmds);
        assert!(resp.error.is_none());
        let entity_id = resp.result.unwrap()["entity_id"].as_u64().unwrap();

        let w = world.lock().unwrap();
        assert!(w.alive.contains(&entity_id));
        let pos = w.get(entity_id, "Position").unwrap();
        assert_eq!(pos["x"], 5);
    }

    #[test]
    fn unknown_method_returns_error() {
        let json = r#"{"jsonrpc":"2.0","method":"nonexistent","id":1}"#;
        let world = Arc::new(Mutex::new(World::new()));
        let cmds = Arc::new(Mutex::new(Vec::new()));
        let resp = handle_request(json, &world, &cmds);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn text_capture_via_rpc() {
        let world = Arc::new(Mutex::new(World::new()));
        {
            let mut w = world.lock().unwrap();
            let e = w.spawn();
            w.insert(e, "Position", json!({"x": 0, "y": 0})).unwrap();
            w.insert(e, "Glyph", json!({"ch": "#"})).unwrap();
        }
        let json = r#"{"jsonrpc":"2.0","method":"render.text_capture","id":1}"#;
        let cmds = Arc::new(Mutex::new(Vec::new()));
        let resp = handle_request(json, &world, &cmds);
        let text = resp.result.unwrap()["text"].as_str().unwrap().to_string();
        assert!(text.contains('#'));
    }
}
