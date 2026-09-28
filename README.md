# nori

A native email client, built twice from one design: a GPUI desktop app in Rust,
and a pixel-matched React/TypeScript web UI.

Both run entirely on mock data for now. The web build is the leading edge — the
Rust side is the eventual host that will embed it and expose the real
filesystem/mail backend.

## Layout

| Path | What it is |
| --- | --- |
| `crates/nori-ui` | Shared GPUI UI crate — views, components, model, theme (legacy) |
| `crates/nori-desktop` | Thin binary host (`nori`), window + icon setup (legacy) |
| `web/` | React + Vite renderer — runs standalone on mock data, or inside Electron |
| `electron/` | Electron + Bun host — window shell + Gmail backend (the port) |

## Electron + Bun (current)

Requires [bun](https://bun.sh). The Electron host in `electron/` ports
`crates/nori-desktop` (window shell, menus, app identity) and
`crates/nori-gmail` (OAuth/PKCE loopback, tokens, Gmail REST, sync,
send, sanitize/policy/rich) to TypeScript. `web/` is the renderer.

```
bun install
bun run dev          # vite :3001 + Electron (dev loads http://localhost:3001)
bun run typecheck    # web + electron
bun run build        # web/dist + electron/dist
bun run test         # bun:test backend parity suite
bun run start        # launch the built app
bun run build:electron  # packaged installers (electron-builder)
```

Credentials live in `.env` at the repo root (see `.env.example`).
Tokens and the mail index stay in Electron `userData` (`0600`).
See [`electron/README.md`](electron/README.md) for the module mapping.

## Desktop (Rust, legacy)

Requires the pinned toolchain in `rust-toolchain.toml` and a Wayland or X11
session. GPUI is pulled from a pinned Zed revision, so the first build is slow.

```
cargo run -p nori-desktop
```

## Web

Requires [bun](https://bun.sh).

```
cd web
bun install
bun run dev        # http://localhost:3001
bun run typecheck
bun run build      # emits web/dist/
```

See [`web/README.md`](web/README.md) for the port mapping between the Rust
types and the TypeScript ones.

## Status

Prototype. No accounts, no sync, no network.
