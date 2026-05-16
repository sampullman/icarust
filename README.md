# Icarust

A 2D Sopwith/Luftrauser-style shoot-'em-up written in Rust on top of
[ggez](https://github.com/ggez/ggez). Pilot a thrust-vector ship with
gravity over a toroidal-X world, dogfight enemy planes, dodge tank
artillery, and ride the difficulty curve as long as you can. The game is
a client/server split: the server runs the authoritative simulation and
clients (native or wasm) connect over a WebSocket.

## Run

```
cargo run -p server                                # binds 127.0.0.1:4015
cargo run -p client                                # connects to ws://127.0.0.1:4015
cargo run -p client -- --connect ws://host:4015 --name alice
cargo test                                         # all crates
```

The client always needs a server running first. Assets load from
`./resources/`; when launched via cargo, `$CARGO_MANIFEST_DIR/../../
resources` is used so working directory doesn't matter.

For the wasm build (browser play through WebGPU), see the `web/` section
in [`CLAUDE.md`](CLAUDE.md).

## Controls

| Key            | Action                                |
| -------------- | ------------------------------------- |
| Left / Right   | rotate ship                           |
| Up             | thrust forward                        |
| Space          | fire (or launch from menu / return from Game Over) |
| Escape         | quit                                  |

## Architecture

See [`CLAUDE.md`](CLAUDE.md) for a tour of the codebase, the workspace
layout, and the conventions that hold across the sim / protocol / server /
client crates. [`server-architecture.md`](server-architecture.md) is the
original migration plan from the single-binary native game to the current
split — useful as history, not a current spec.
