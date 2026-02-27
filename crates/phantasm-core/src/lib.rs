use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type EntityId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentSchema {
    pub name: String,
    pub fields: BTreeMap<String, FieldType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldType {
    Float,
    Int,
    String,
    Bool,
    Object,
    Array,
}

#[derive(Debug)]
pub enum EcsError {
    EntityNotFound(EntityId),
    InvalidData(String),
}

impl std::fmt::Display for EcsError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::EntityNotFound(id) => write!(f, "entity {} not found", id),
            Self::InvalidData(msg) => write!(f, "invalid data: {}", msg),
        }
    }
}

impl std::error::Error for EcsError {}

pub struct World {
    next_entity: EntityId,
    pub alive: HashSet<EntityId>,
    pub storage: HashMap<String, HashMap<EntityId, Value>>,
    pub schemas: HashMap<String, ComponentSchema>,
    pub frame: u64,
}

impl World {
    pub fn new() -> Self {
        let mut world = Self {
            next_entity: 1,
            alive: HashSet::new(),
            storage: HashMap::new(),
            schemas: HashMap::new(),
            frame: 0,
        };
        world.register_builtins();
        world
    }

    fn register_builtins(&mut self) {
        self.register_schema(ComponentSchema {
            name: "Position".into(),
            fields: BTreeMap::from([
                ("x".into(), FieldType::Float),
                ("y".into(), FieldType::Float),
            ]),
        });
        self.register_schema(ComponentSchema {
            name: "Glyph".into(),
            fields: BTreeMap::from([
                ("ch".into(), FieldType::String),
                ("fg".into(), FieldType::String),
                ("bg".into(), FieldType::String),
            ]),
        });
        self.register_schema(ComponentSchema {
            name: "Name".into(),
            fields: BTreeMap::from([("name".into(), FieldType::String)]),
        });
        self.register_schema(ComponentSchema {
            name: "Health".into(),
            fields: BTreeMap::from([
                ("current".into(), FieldType::Float),
                ("max".into(), FieldType::Float),
            ]),
        });
        self.register_schema(ComponentSchema {
            name: "Velocity".into(),
            fields: BTreeMap::from([
                ("dx".into(), FieldType::Float),
                ("dy".into(), FieldType::Float),
            ]),
        });
        self.register_schema(ComponentSchema {
            name: "Collider".into(),
            fields: BTreeMap::from([("solid".into(), FieldType::Bool)]),
        });
        self.register_schema(ComponentSchema {
            name: "Player".into(),
            fields: BTreeMap::new(),
        });
        self.register_schema(ComponentSchema {
            name: "Wall".into(),
            fields: BTreeMap::new(),
        });
    }

    pub fn register_schema(&mut self, schema: ComponentSchema) {
        self.storage.entry(schema.name.clone()).or_default();
        self.schemas.insert(schema.name.clone(), schema);
    }

    pub fn spawn(&mut self) -> EntityId {
        let id = self.next_entity;
        self.next_entity += 1;
        self.alive.insert(id);
        id
    }

    pub fn despawn(&mut self, entity: EntityId) -> bool {
        if !self.alive.remove(&entity) {
            return false;
        }
        for storage in self.storage.values_mut() {
            storage.remove(&entity);
        }
        true
    }

    pub fn insert(
        &mut self,
        entity: EntityId,
        component: &str,
        data: Value,
    ) -> Result<(), EcsError> {
        if !self.alive.contains(&entity) {
            return Err(EcsError::EntityNotFound(entity));
        }
        if !self.schemas.contains_key(component) {
            self.register_schema(ComponentSchema {
                name: component.to_string(),
                fields: BTreeMap::new(),
            });
        }
        self.storage
            .entry(component.to_string())
            .or_default()
            .insert(entity, data);
        Ok(())
    }

    pub fn get(&self, entity: EntityId, component: &str) -> Option<&Value> {
        self.storage.get(component)?.get(&entity)
    }

    pub fn remove(&mut self, entity: EntityId, component: &str) -> Option<Value> {
        self.storage.get_mut(component)?.remove(&entity)
    }

    pub fn query(&self, components: &[&str]) -> Vec<EntityId> {
        let mut result: Vec<EntityId> = self.alive.iter().copied().collect();
        for comp in components {
            if let Some(storage) = self.storage.get(*comp) {
                result.retain(|id| storage.contains_key(id));
            } else {
                return vec![];
            }
        }
        result.sort();
        result
    }

    pub fn entity_components(&self, entity: EntityId) -> HashMap<String, Value> {
        let mut result = HashMap::new();
        for (name, storage) in &self.storage {
            if let Some(data) = storage.get(&entity) {
                result.insert(name.clone(), data.clone());
            }
        }
        result
    }

    pub fn snapshot(&self) -> Value {
        let mut entities = serde_json::Map::new();
        let mut sorted_entities: Vec<EntityId> = self.alive.iter().copied().collect();
        sorted_entities.sort();

        for entity in sorted_entities {
            let components = self.entity_components(entity);
            let comp_map: serde_json::Map<String, Value> = components.into_iter().collect();
            entities.insert(entity.to_string(), Value::Object(comp_map));
        }

        let schemas: BTreeMap<&String, &ComponentSchema> = self.schemas.iter().collect();

        serde_json::json!({
            "entities": entities,
            "next_entity": self.next_entity,
            "frame": self.frame,
            "schemas": schemas,
        })
    }

    pub fn load_snapshot(&mut self, snapshot: &Value) -> Result<(), EcsError> {
        self.alive.clear();
        for storage in self.storage.values_mut() {
            storage.clear();
        }

        if let Some(next) = snapshot.get("next_entity").and_then(|v| v.as_u64()) {
            self.next_entity = next;
        }
        if let Some(frame) = snapshot.get("frame").and_then(|v| v.as_u64()) {
            self.frame = frame;
        }

        if let Some(entities) = snapshot.get("entities").and_then(|v| v.as_object()) {
            for (id_str, components) in entities {
                let entity_id: EntityId = id_str
                    .parse()
                    .map_err(|e: std::num::ParseIntError| EcsError::InvalidData(e.to_string()))?;
                self.alive.insert(entity_id);
                if self.next_entity <= entity_id {
                    self.next_entity = entity_id + 1;
                }
                if let Some(comp_map) = components.as_object() {
                    for (comp_name, data) in comp_map {
                        self.storage
                            .entry(comp_name.clone())
                            .or_default()
                            .insert(entity_id, data.clone());
                    }
                }
            }
        }
        Ok(())
    }

    pub fn load_scene(&mut self, scene_json: &str) -> Result<(), EcsError> {
        let scene: Value =
            serde_json::from_str(scene_json).map_err(|e| EcsError::InvalidData(e.to_string()))?;
        if let Some(entities) = scene.get("entities").and_then(|v| v.as_array()) {
            for entity_data in entities {
                if let Some(components) = entity_data.as_object() {
                    let entity = self.spawn();
                    for (comp_name, data) in components {
                        self.insert(entity, comp_name, data.clone())?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn text_capture(&self) -> String {
        let entities = self.query(&["Position", "Glyph"]);
        if entities.is_empty() {
            return String::new();
        }

        let mut max_x: i64 = 0;
        let mut max_y: i64 = 0;
        for &eid in &entities {
            if let Some(pos) = self.get(eid, "Position") {
                let x = pos.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
                let y = pos.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64;
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }

        let width = (max_x + 1) as usize;
        let height = (max_y + 1) as usize;
        let mut grid = vec![vec![' '; width]; height];

        for &eid in &entities {
            if let (Some(pos), Some(glyph)) = (self.get(eid, "Position"), self.get(eid, "Glyph")) {
                let x = pos.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as usize;
                let y = pos.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as usize;
                let ch = glyph
                    .get("ch")
                    .and_then(|v| v.as_str())
                    .unwrap_or(" ")
                    .chars()
                    .next()
                    .unwrap_or(' ');
                if y < height && x < width {
                    grid[y][x] = ch;
                }
            }
        }

        grid.iter()
            .map(|row| row.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_and_insert() {
        let mut world = World::new();
        let e = world.spawn();
        world
            .insert(e, "Position", serde_json::json!({"x": 1.0, "y": 2.0}))
            .unwrap();
        assert_eq!(
            world.get(e, "Position"),
            Some(&serde_json::json!({"x": 1.0, "y": 2.0}))
        );
    }

    #[test]
    fn query_filters() {
        let mut world = World::new();
        let e1 = world.spawn();
        let e2 = world.spawn();
        world
            .insert(e1, "Position", serde_json::json!({"x": 0, "y": 0}))
            .unwrap();
        world.insert(e1, "Player", serde_json::json!({})).unwrap();
        world
            .insert(e2, "Position", serde_json::json!({"x": 1, "y": 1}))
            .unwrap();

        assert_eq!(world.query(&["Position", "Player"]), vec![e1]);
        let with_pos = world.query(&["Position"]);
        assert!(with_pos.contains(&e1) && with_pos.contains(&e2));
        assert_eq!(with_pos.len(), 2);
    }

    #[test]
    fn snapshot_roundtrip() {
        let mut world = World::new();
        let e = world.spawn();
        world
            .insert(e, "Position", serde_json::json!({"x": 1.0, "y": 2.0}))
            .unwrap();
        world
            .insert(e, "Name", serde_json::json!({"name": "Test"}))
            .unwrap();

        let snapshot = world.snapshot();
        let mut world2 = World::new();
        world2.load_snapshot(&snapshot).unwrap();

        assert_eq!(
            world2.get(e, "Position"),
            Some(&serde_json::json!({"x": 1.0, "y": 2.0}))
        );
        assert_eq!(
            world2.get(e, "Name"),
            Some(&serde_json::json!({"name": "Test"}))
        );
    }

    #[test]
    fn despawn_removes_components() {
        let mut world = World::new();
        let e = world.spawn();
        world.insert(e, "Position", serde_json::json!({})).unwrap();
        assert!(world.despawn(e));
        assert_eq!(world.get(e, "Position"), None);
        assert!(!world.despawn(e));
    }

    #[test]
    fn load_scene_from_json() {
        let mut world = World::new();
        let scene = r#"{"entities": [{"Position": {"x": 5, "y": 10}, "Player": {}}]}"#;
        world.load_scene(scene).unwrap();
        let players = world.query(&["Position", "Player"]);
        assert_eq!(players.len(), 1);
        let pos = world.get(players[0], "Position").unwrap();
        assert_eq!(pos.get("x").unwrap().as_i64(), Some(5));
    }

    #[test]
    fn text_capture_renders_grid() {
        let mut world = World::new();
        let e = world.spawn();
        world
            .insert(e, "Position", serde_json::json!({"x": 2, "y": 1}))
            .unwrap();
        world
            .insert(e, "Glyph", serde_json::json!({"ch": "@"}))
            .unwrap();
        let capture = world.text_capture();
        let lines: Vec<&str> = capture.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].chars().nth(2), Some('@'));
    }

    #[test]
    fn auto_registers_unknown_components() {
        let mut world = World::new();
        let e = world.spawn();
        world
            .insert(e, "CustomComponent", serde_json::json!({"foo": "bar"}))
            .unwrap();
        assert!(world.schemas.contains_key("CustomComponent"));
    }
}
