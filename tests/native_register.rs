use std::collections::BTreeSet;

use bevy::prelude::{App, Update};
use serde_json::json;
use swarm_engine_api::ids::RoomId;
use swarm_engine_api::prelude::WorldMode;
use swarm_engine_plugin_sdk::native::{
    NativeModConfig, NativeModInstallExpectation, NativeModRegisterContext, NativeModRegisterError,
};
use swarm_engine_plugin_sdk::prelude::Position;
use swarm_engine_plugin_sdk::resources::InstalledPluginDescriptors;
use swarm_engine_plugin_sdk::traits::SwarmPlugin;
use swarm_mod_fog_of_war::{
    FogOfWarModPlugin, PlayerViewMode, PositionKey, VisibilityConfig, VisibilityMap, is_visible_to,
    register,
};

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
    let mut app = App::new();
    let entity = app.world_mut().spawn_empty().id();

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
    let mut app = App::new();
    let mut context = NativeModRegisterContext::new(
        &mut app,
        "fog-of-war",
        WorldMode::Default,
        NativeModConfig::from_defaults(json!({
            "fog_of_war": false,
            "player_view": "full"
        })),
        NativeModInstallExpectation::enabled("0.1.0"),
    );

    register(&mut context).expect("register fog-of-war");
    drop(context);

    assert!(!app.world().resource::<VisibilityConfig>().fog_of_war);
    assert_eq!(
        app.world().resource::<VisibilityConfig>().player_view,
        PlayerViewMode::Full
    );
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

#[test]
fn native_register_rejects_unknown_fields_without_installing_the_plugin() {
    let mut app = App::new();
    let mut context = NativeModRegisterContext::new(
        &mut app,
        "fog-of-war",
        WorldMode::Default,
        NativeModConfig::from_defaults(json!({
            "fog_of_war": true,
            "player_view": "drone",
            "unexpected": true
        })),
        NativeModInstallExpectation::enabled("0.1.0"),
    );

    let error = register(&mut context).expect_err("unknown config field must fail registration");

    assert!(matches!(
        error,
        NativeModRegisterError::InvalidConfig { .. }
    ));
    assert!(app.world().get_resource::<VisibilityConfig>().is_none());
    assert!(
        app.world()
            .get_resource::<InstalledPluginDescriptors>()
            .is_none()
    );
}
