# nori

A native email client, built twice from one design: a GPUI desktop app in Rust,
and a pixel-matched React/TypeScript web UI.

Both run entirely on mock data for now. The web build is the leading edge — the
Rust side is the eventual host that will embed it and expose the real
filesystem/mail backend.

## Layout

| Path | What it is |
| --- | --- |
| `crates/nori-ui` | Shared GPUI UI crate — views, components, model, theme |
| `crates/nori-desktop` | Thin binary host (`nori`), window + icon setup |
| `web/` | Standalone React + Vite port of the GPUI UI |

## Desktop (Rust)

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
