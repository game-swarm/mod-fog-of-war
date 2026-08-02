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
    pub player_view: PlayerViewMode,
}

impl Default for VisibilityConfig {
    fn default() -> Self {
        Self {
            fog_of_war: true,
            player_view: PlayerViewMode::Drone,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlayerViewMode {
    #[default]
    Drone,
    Full,
    Allied,
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
#[serde(deny_unknown_fields)]
struct FogOfWarRegisterConfig {
    fog_of_war: bool,
    player_view: PlayerViewMode,
}

pub fn register(context: &mut NativeModRegisterContext<'_>) -> Result<(), NativeModRegisterError> {
    let config = context.decode_config::<FogOfWarRegisterConfig>()?;
    context.install(ConfiguredFogOfWarModPlugin {
        config: VisibilityConfig {
            fog_of_war: config.fog_of_war,
            player_view: config.player_view,
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
