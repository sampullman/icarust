# Icarust

A 2D Sopwith/Luftrauser-style shoot-'em-up written in Rust on top of [ggez](https://github.com/ggez/ggez). Pilot a thrust-vector ship with gravity and toroidal world wrap; clear levels by shooting all the rocks.

## Run

```
cargo run            # debug
cargo run --release  # smoother physics
cargo test           # unit tests
```

The game loads assets from `./resources/`; when launched via cargo, `$CARGO_MANIFEST_DIR/resources` is used so working directory doesn't matter.

## Controls

| Key            | Action                  |
| -------------- | ----------------------- |
| Left / Right   | rotate ship             |
| Up             | thrust forward          |
| Space          | fire                    |
| R              | restart after game over |
| Escape         | quit                    |

## Architecture

See [`CLAUDE.md`](CLAUDE.md) for a tour of the codebase and conventions, and [`improvements.md`](improvements.md) for tracked bugs and cleanup ideas.
