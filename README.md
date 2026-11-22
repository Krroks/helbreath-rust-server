# Helbreath server (Rust)

A Rust port of the Helbreath International servers: the login server and the game server. It also includes a
WebSocket bridge so the browser client ([helbreath-web-client](https://github.com/Krroks/helbreath-web-client)) can
reach them. It speaks the original protocol, so the original Windows client works with it too.

| Crate | What it is |
|---|---|
| `proto` | packet framing and the original cipher |
| `login` | login / gate server: accounts and characters (MySQL), configuration for the game servers |
| `hg` | game server: maps, players, NPCs, combat, magic, items, events |
| `bridge` | WebSocket <-> TCP bridge for the browser client (and an optional TCP proxy that records traffic) |

## Requirements

- Rust stable (edition 2021): `rustup` from https://rustup.rs
- MySQL or MariaDB with the Helbreath International database. The schema is `Run_Me.sql` from the Helbreath
  International source; it is not in this repository.
- Server data, also not included:
  - for `login`: `LServer.cfg` and a `Config/` folder with the item, NPC, magic, skill, quest, potion and
    crafting `.cfg` files that it sends to the game servers;
  - for `hg`: a folder with `GServer.cfg`, `Settings.cfg` and `MAPDATA/` (the `.amd` maps and their `.txt` info
    files).

## Build and test

```sh
cargo build --release
cargo test
```

The binaries end up in `target/release/`: `login`, `hg`, `bridge`.

## Run

Start them in this order.

### 1. Login server

```sh
login --config path/to/LServer.cfg [--config-dir path/to/Config] [--bind 0.0.0.0] \
      [--tcp-port 4000] [--ws-port PORT] [--gs-port 5656] [--no-db-maintenance]
```

- `LServer.cfg` holds the listen port and the database connection (`mysql-address`, `mysql-server-port`,
  `mysql-user`, `mysql-password`).
- Clients connect over WebSocket on the port in `LServer.cfg` (or `--ws-port`), and over plain TCP only when
  `--tcp-port` is given. The Windows client and the bridge use TCP.
- Game servers register on `--gs-port` (5656 by default).
- `--config-dir` defaults to `Config/` next to `LServer.cfg`.

### 2. Game server

```sh
hg --dir path/to/GameServer [--bind 0.0.0.0] [--login HOST:5656] [--local-ip IP] [--ws-port PORT] [--no-tcp]
```

- `--dir` is the folder with `GServer.cfg`, `Settings.cfg` and `MAPDATA/`.
- `GServer.cfg` names the server, its maps, its port, and the address of the login server. `--login` overrides that
  address.
- The game server registers with the login server at start ("Game Server registration to Log Server - Success!").
- Lines typed on its standard input are read as console commands.

### 3. Bridge (only for the browser client)

```sh
bridge --listen 127.0.0.1:8000 --allow-host LOGIN_HOST [--allow-host GAME_HOST ...] [--record trace.jsonl]
```

The browser opens `ws://<bridge>/<host>:<port>`. Each WebSocket message carries one protocol frame. Only hosts
given with `--allow-host` are reachable, and the WebSocket listener does not start without at least one.
`--tcp LISTEN=TARGET` adds a plain TCP proxy (for recording the Windows client), and `--record` writes every
deciphered frame as one JSON line.

## Map window (16:9)

The client shows 854x480, so the map window the game server sends around a player is 28 columns wide, starting
13 columns to the player's left (the original was 21 columns from 10 to the left), and 16 rows. Movement and NPC
events reach players up to 15 columns away. The values are `VIEW_LEFT`, `VIEW_W`, `VIEW_NEAR_X` and `VIEW_NEAR_Y` in
`hg/src/consts.rs`. A 4:3 client gets its first map tiles misplaced by this server, so use the 16:9 client.

## Probes

`login/examples/probe.rs` and `hg/examples/hgprobe.rs` play scripted sessions against running servers, using test
accounts they create (password `rtpw1`). `hg/PORTING.md` records how the game server was ported and what each probe
checks.
