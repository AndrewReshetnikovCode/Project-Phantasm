use mlua::prelude::*;
use phantasm_core::World;
use serde_json::Value;

fn lua_err(e: mlua::Error) -> anyhow::Error {
    anyhow::anyhow!("{}", e)
}

const PHANTASM_STDLIB: &str = r#"
_commands = {}

function cmd_set(entity_id, component, data)
    table.insert(_commands, {type = "set", entity = entity_id, component = component, data = data})
end

function cmd_spawn(components)
    table.insert(_commands, {type = "spawn", components = components})
end

function cmd_despawn(entity_id)
    table.insert(_commands, {type = "despawn", entity = entity_id})
end

function cmd_log(msg)
    table.insert(_commands, {type = "log", message = tostring(msg)})
end

function query(...)
    local component_names = {...}
    local result = {}
    if not _world or not _world.entities then return result end

    for entity_id, components in pairs(_world.entities) do
        local has_all = true
        local entry = {id = entity_id}
        for _, name in ipairs(component_names) do
            if components[name] ~= nil then
                entry[name] = components[name]
            else
                has_all = false
                break
            end
        end
        if has_all then
            table.insert(result, entry)
        end
    end

    return result
end
"#;

pub struct ScriptEngine {
    lua: Lua,
    pub logs: Vec<String>,
}

impl ScriptEngine {
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();
        lua.load(PHANTASM_STDLIB).exec()?;

        Ok(Self {
            lua,
            logs: Vec::new(),
        })
    }

    pub fn load_script(&mut self, source: &str) -> LuaResult<()> {
        self.lua.load(source).exec()?;
        Ok(())
    }

    pub fn call_init(&mut self, world: &mut World) -> anyhow::Result<()> {
        self.prepare_world_data(world)?;
        self.lua
            .load("_commands = {}; if on_init then on_init() end")
            .exec()
            .map_err(lua_err)?;
        self.process_commands(world)?;
        Ok(())
    }

    pub fn call_update(
        &mut self,
        world: &mut World,
        dt: f64,
        pressed_actions: &[String],
    ) -> anyhow::Result<()> {
        self.prepare_world_data(world)?;
        self.lua.globals().set("_dt", dt).map_err(lua_err)?;

        let input_table = self.lua.create_table().map_err(lua_err)?;
        let pressed_table = self.lua.create_table().map_err(lua_err)?;
        for action in pressed_actions {
            pressed_table.set(action.as_str(), true).map_err(lua_err)?;
        }
        input_table.set("pressed", pressed_table).map_err(lua_err)?;
        self.lua
            .globals()
            .set("_input", input_table)
            .map_err(lua_err)?;

        self.lua
            .load("_commands = {}; if on_update then on_update(_dt) end")
            .exec()
            .map_err(lua_err)?;
        self.process_commands(world)?;
        Ok(())
    }

    fn prepare_world_data(&self, world: &World) -> anyhow::Result<()> {
        let world_table = self.lua.create_table().map_err(lua_err)?;
        let entities_table = self.lua.create_table().map_err(lua_err)?;

        for &entity_id in &world.alive {
            let comp_table = self.lua.create_table().map_err(lua_err)?;
            let components = world.entity_components(entity_id);
            for (name, value) in components {
                let lua_value: LuaValue = self.lua.to_value(&value).map_err(lua_err)?;
                comp_table.set(name.as_str(), lua_value).map_err(lua_err)?;
            }
            entities_table.set(entity_id, comp_table).map_err(lua_err)?;
        }

        world_table
            .set("entities", entities_table)
            .map_err(lua_err)?;
        self.lua
            .globals()
            .set("_world", world_table)
            .map_err(lua_err)?;
        Ok(())
    }

    fn process_commands(&mut self, world: &mut World) -> anyhow::Result<()> {
        let commands_table: LuaTable = self.lua.globals().get("_commands").map_err(lua_err)?;

        for pair in commands_table.pairs::<i64, LuaTable>() {
            let (_, cmd) = pair.map_err(lua_err)?;
            let cmd_type: String = cmd.get("type").map_err(lua_err)?;

            match cmd_type.as_str() {
                "set" => {
                    let entity_id: u64 = cmd.get("entity").map_err(lua_err)?;
                    let comp_name: String = cmd.get("component").map_err(lua_err)?;
                    let data: LuaValue = cmd.get("data").map_err(lua_err)?;
                    let json_data: Value = self.lua.from_value(data).map_err(lua_err)?;
                    world.insert(entity_id, &comp_name, json_data)?;
                }
                "spawn" => {
                    let components: LuaTable = cmd.get("components").map_err(lua_err)?;
                    let entity = world.spawn();
                    for pair in components.pairs::<String, LuaValue>() {
                        let (name, value) = pair.map_err(lua_err)?;
                        let json_data: Value = self.lua.from_value(value).map_err(lua_err)?;
                        world.insert(entity, &name, json_data)?;
                    }
                }
                "despawn" => {
                    let entity_id: u64 = cmd.get("entity").map_err(lua_err)?;
                    world.despawn(entity_id);
                }
                "log" => {
                    let message: String = cmd.get("message").map_err(lua_err)?;
                    self.logs.push(message);
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn drain_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdlib_loads() {
        let engine = ScriptEngine::new().unwrap();
        assert!(engine.logs.is_empty());
    }

    #[test]
    fn spawn_via_script() {
        let mut engine = ScriptEngine::new().unwrap();
        let mut world = World::new();

        engine
            .load_script(
                r#"
            function on_init()
                cmd_spawn({
                    Position = {x = 10, y = 20},
                    Name = {name = "ScriptEntity"}
                })
                cmd_log("Entity spawned!")
            end
        "#,
            )
            .unwrap();

        engine.call_init(&mut world).unwrap();

        let entities = world.query(&["Position", "Name"]);
        assert_eq!(entities.len(), 1);

        let pos = world.get(entities[0], "Position").unwrap();
        assert_eq!(pos.get("x").unwrap().as_f64(), Some(10.0));

        let logs = engine.drain_logs();
        assert_eq!(logs, vec!["Entity spawned!"]);
    }

    #[test]
    fn update_modifies_entity() {
        let mut engine = ScriptEngine::new().unwrap();
        let mut world = World::new();

        let e = world.spawn();
        world
            .insert(e, "Position", serde_json::json!({"x": 0.0, "y": 0.0}))
            .unwrap();
        world.insert(e, "Player", serde_json::json!({})).unwrap();

        engine
            .load_script(
                r#"
            function on_update(dt)
                local players = query("Position", "Player")
                for _, p in ipairs(players) do
                    if _input and _input.pressed and _input.pressed["move_right"] then
                        cmd_set(p.id, "Position", {x = p.Position.x + 1, y = p.Position.y})
                    end
                end
            end
        "#,
            )
            .unwrap();

        engine
            .call_update(&mut world, 0.033, &["move_right".to_string()])
            .unwrap();

        let pos = world.get(e, "Position").unwrap();
        assert_eq!(pos.get("x").unwrap().as_f64(), Some(1.0));
    }
}
