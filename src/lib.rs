use bevy::prelude::*;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use swarm_engine_api::prelude::{
    API_VERSION, ConfigFieldDescriptor, ConfigValueType, DESCRIPTOR_SCHEMA_VERSION, PlayerId,
    PluginDescriptor, SystemDescriptor, TickPhase,
};
use swarm_engine_plugin_sdk::native::{NativeModRegisterContext, NativeModRegisterError};
use swarm_engine_plugin_sdk::prelude::{
    Controller, Drone, Owner, Position, Structure, StructureType,
};
use swarm_engine_plugin_sdk::traits::SwarmPlugin;

#[derive(Resource, Debug, Clone)]
pub struct VisibilityConfig {
    pub fog_of_war: bool,
}

impl Default for VisibilityConfig {
    fn default() -> Self {
        Self { fog_of_war: true }
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct VisibilityMap {
    pub visible_entities: BTreeMap<PlayerId, BTreeSet<Entity>>,
    pub visible_positions: BTreeMap<PlayerId, BTreeSet<PositionKey>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PositionKey {
    pub room: u32,
    pub x: i32,
    pub y: i32,
}

impl From<Position> for PositionKey {
    fn from(position: Position) -> Self {
        Self {
            room: position.room.0,
            x: position.x,
            y: position.y,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FogOfWarModPlugin;

impl Plugin for FogOfWarModPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VisibilityConfig>()
            .init_resource::<VisibilityMap>()
            .add_systems(Update, visibility_snapshot_system);
    }
}

impl SwarmPlugin for FogOfWarModPlugin {
    fn descriptor() -> PluginDescriptor {
        PluginDescriptor {
            id: "fog-of-war".to_string(),
            version: "0.1.0".to_string(),
            api_version: API_VERSION.to_string(),
            dependencies: Vec::new(),
            config: vec![
                ConfigFieldDescriptor {
                    key: "fog_of_war".to_string(),
                    value_type: ConfigValueType::Bool,
                    default: true.into(),
                    required: false,
                    validator: None,
                },
                ConfigFieldDescriptor {
                    key: "player_view".to_string(),
                    value_type: ConfigValueType::Enum {
                        values: ["drone", "full", "allied"].map(str::to_string).to_vec(),
                    },
                    default: "drone".into(),
                    required: false,
                    validator: None,
                },
            ],
            systems: vec![SystemDescriptor {
                system_id: "fog-of-war.snapshot".to_string(),
                version: "0.1.0".to_string(),
                phase: TickPhase::Update,
                order: 0,
                reads: vec![
                    "VisibilityConfig".to_string(),
                    "Position".to_string(),
                    "Drone".to_string(),
                    "Structure".to_string(),
                    "Controller".to_string(),
                    "Owner".to_string(),
                ],
                writes: vec!["VisibilityMap".to_string()],
                produces_buffers: Vec::new(),
                consumes_buffers: Vec::new(),
                deterministic_iteration: vec!["PlayerId".to_string()],
            }],
            actions: Vec::new(),
            descriptor_schema_version: DESCRIPTOR_SCHEMA_VERSION.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfiguredFogOfWarModPlugin {
    config: VisibilityConfig,
}

impl Plugin for ConfiguredFogOfWarModPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone());
        FogOfWarModPlugin.build(app);
    }
}

impl SwarmPlugin for ConfiguredFogOfWarModPlugin {
    fn descriptor() -> PluginDescriptor {
        FogOfWarModPlugin::descriptor()
    }

    fn register(app: &mut App) {
        FogOfWarModPlugin::register(app);
    }
}

#[derive(Debug, Deserialize)]
struct FogOfWarRegisterConfig {
    fog_of_war: bool,
    player_view: String,
}

pub fn register(context: &mut NativeModRegisterContext<'_>) -> Result<(), NativeModRegisterError> {
    let config = context.decode_config::<FogOfWarRegisterConfig>()?;
    let _player_view = config.player_view;
    context.install(ConfiguredFogOfWarModPlugin {
        config: VisibilityConfig {
            fog_of_war: config.fog_of_war,
        },
    })
}

pub fn visibility_snapshot_system(
    config: Res<VisibilityConfig>,
    mut map: ResMut<VisibilityMap>,
    all_entities: Query<(Entity, Option<&Position>)>,
    drones: Query<(&Drone, &Position)>,
    structures: Query<(&Structure, &Position)>,
    controllers: Query<(&Controller, &Position)>,
    owners: Query<(&Owner, &Position)>,
) {
    map.visible_entities.clear();
    map.visible_positions.clear();

    let mut players = BTreeSet::new();
    for (drone, _) in &drones {
        players.insert(drone.owner);
    }
    for (structure, _) in &structures {
        if let Some(owner) = structure.owner {
            players.insert(owner);
        }
    }
    for (controller, _) in &controllers {
        if let Some(owner) = controller.owner {
            players.insert(owner);
        }
    }
    for (owner, _) in &owners {
        players.insert(owner.0);
    }

    let all_position_set: BTreeSet<_> = all_entities
        .iter()
        .filter_map(|(_, position)| position.copied().map(PositionKey::from))
        .collect();

    for player in players {
        let visible_positions = if config.fog_of_war {
            player_visible_positions(player, &drones, &structures, &controllers, &owners)
        } else {
            all_position_set.clone()
        };

        let visible_entities = all_entities
            .iter()
            .filter_map(|(entity, position)| {
                position
                    .is_some_and(|position| {
                        visible_positions.contains(&PositionKey::from(*position))
                    })
                    .then_some(entity)
            })
            .collect();
        map.visible_positions.insert(player, visible_positions);
        map.visible_entities.insert(player, visible_entities);
    }
}

pub fn is_visible_to(map: &VisibilityMap, player: PlayerId, entity: Entity) -> bool {
    map.visible_entities
        .get(&player)
        .is_some_and(|visible| visible.contains(&entity))
}

fn player_visible_positions(
    player: PlayerId,
    drones: &Query<(&Drone, &Position)>,
    structures: &Query<(&Structure, &Position)>,
    controllers: &Query<(&Controller, &Position)>,
    owners: &Query<(&Owner, &Position)>,
) -> BTreeSet<PositionKey> {
    let mut anchors = Vec::new();
    let mut room_radius = 1u32;

    for (drone, position) in drones {
        if drone.owner == player {
            anchors.push((*position, 1));
        }
    }
    for (structure, position) in structures {
        if structure.owner == Some(player) {
            let radius = if structure.structure_type == StructureType::OBSERVER {
                2
            } else {
                1
            };
            anchors.push((*position, radius));
            room_radius = room_radius.max(radius);
        }
    }
    for (controller, position) in controllers {
        if controller.owner == Some(player) {
            let radius = if controller.level >= 5 {
                1 + (controller.level - 4) as u32
            } else {
                1
            };
            anchors.push((*position, radius));
            room_radius = room_radius.max(radius);
        }
    }
    for (owner, position) in owners {
        if owner.0 == player {
            anchors.push((*position, room_radius));
        }
    }

    let mut visible = BTreeSet::new();
    for (anchor, radius) in anchors {
        for dy in -(radius as i32)..=(radius as i32) {
            for dx in -(radius as i32)..=(radius as i32) {
                if let Some(room) = anchor.room.adjacent(dx, dy) {
                    visible.insert(PositionKey::from(Position {
                        x: anchor.x,
                        y: anchor.y,
                        room,
                    }));
                }
            }
        }
    }
    visible
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarm_engine_api::ids::RoomId;

    #[test]
    fn position_key_preserves_room_and_coordinates() {
        let key = PositionKey::from(Position {
            x: -3,
            y: 9,
            room: RoomId(42),
        });

        assert_eq!(key.room, 42);
        assert_eq!(key.x, -3);
        assert_eq!(key.y, 9);
    }

    #[test]
    fn missing_visibility_entry_is_not_visible() {
        let map = VisibilityMap::default();
        let mut world = World::new();
        let entity = world.spawn_empty().id();

        assert!(!is_visible_to(&map, 1, entity));
    }

    #[test]
    fn descriptor_is_valid_and_identifies_fog_of_war() {
        let descriptor = FogOfWarModPlugin::descriptor();
        swarm_engine_api::validation::assert_valid_descriptor(&descriptor);
        assert_eq!(descriptor.id, "fog-of-war");
        assert_eq!(descriptor.config.len(), 2);
        assert_eq!(descriptor.systems.len(), 1);
        assert_eq!(
            descriptor
                .config
                .iter()
                .map(|field| field.key.as_str())
                .collect::<Vec<_>>(),
            ["fog_of_war", "player_view"]
        );
        assert_eq!(descriptor.systems[0].system_id, "fog-of-war.snapshot");
        assert!(descriptor.dependencies.is_empty());
    }

    #[test]
    fn native_register_preserves_descriptor_resource_and_system_behavior() {
        use swarm_engine_api::prelude::WorldMode;
        use swarm_engine_plugin_sdk::native::{
            NativeModConfig, NativeModInstallExpectation, NativeModRegisterContext,
        };
        use swarm_engine_plugin_sdk::resources::InstalledPluginDescriptors;

        let mut app = App::new();
        let mut context = NativeModRegisterContext::new(
            &mut app,
            "fog-of-war",
            WorldMode::Default,
            NativeModConfig::from_defaults(serde_json::json!({
                "fog_of_war": false,
                "player_view": "full"
            })),
            NativeModInstallExpectation::enabled("0.1.0"),
        );

        register(&mut context).expect("register fog-of-war");
        drop(context);

        assert!(!app.world().resource::<VisibilityConfig>().fog_of_war);
        assert_eq!(
            app.world()
                .resource::<InstalledPluginDescriptors>()
                .get("fog-of-war"),
            Some(&FogOfWarModPlugin::descriptor())
        );
        app.world_mut()
            .resource_mut::<VisibilityMap>()
            .visible_positions
            .insert(
                42,
                BTreeSet::from([PositionKey {
                    room: 0,
                    x: 1,
                    y: 2,
                }]),
            );
        app.world_mut().run_schedule(Update);
        assert!(
            app.world()
                .resource::<VisibilityMap>()
                .visible_positions
                .is_empty()
        );
    }
}
