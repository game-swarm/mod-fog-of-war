# Swarm Mod: fog-of-war

Vision and visibility control — fog of war, player view, and spectator modes for Swarm
bool
bool
bool

## Directory Structure

```
mods/fog-of-war/
├── Cargo.toml        # Static Bevy Plugin crate
├── mod.toml          # Mod metadata + configurable parameters
├── src/lib.rs        # `impl Plugin` entry point
└── README.md
```

## Configuration

See `mod.toml` for all configurable parameters. Server operators can override via:

```bash
swarm mod config fog-of-war <key> <value>
```

Or in `world.toml`:

```toml
[mods.fog-of-war.config]
# key = value
```

## Engine API

Mods are statically compiled Bevy Plugin crates. Enable this mod with the
`mod_fog_of_war` Cargo feature, or with `vanilla_mods`.

## Publishing

```bash
git tag v0.1.0
git push --tags
swarm mod pack
```
