# Nori — Electron shell

Electron + Bun host for the Nori email client. Port of `crates/nori-desktop`
(window shell) and `crates/nori-gmail` (Gmail backend) to TypeScript running
under Bun/Electron. The renderer is unchanged `web/` (React + Vite).

## Layout

- `src/main.ts` — window shell (1200x760, min 760x520, `dev.nori.prototype`),
  menus, IPC handlers, 30s sync poll, disk-cache resume.
- `src/preload.ts` — context-isolated `window.nori` bridge.
- `src/ipc.ts` — versioned bridge types (`NORI_PROTOCOL_VERSION = 1`).
- `src/backend/auth/` — PKCE (`pkce.ts`), installed-app OAuth + loopback
  (`oauth.ts` via `Bun.serve`), tokens + `.env` discovery (`token.ts`).
- `src/backend/gmail/` — `http.ts` errors, `gmail.ts` REST client,
  `sync.ts` orchestrator, `syncBodies.ts` MIME parsing, `counts.ts`,
  `send.ts` MIME builder, `sanitize.ts`, `policy.ts`, `rich.ts`,
  `stream.ts`, `cache.ts` index, `account.ts` mailbox mapping.
- `src/backend/settings.ts` — `settings.json` store.
- `tests/backend.test.ts` — `bun test` parity tests (PKCE vector, OAuth,
  HTTP errors, send, counts, cache veto, sanitize/policy, rich).

## Run

```
bun install
bun run dev:web       # renderer on http://localhost:3001
bun --cwd electron run build:main && electron electron/dist/main.js --dev
# or from the repo root:
bun run dev
bun run build         # web/dist + electron/dist
bun run test
```

Credentials live in `.env` at the repo root (`NORI_GMAIL_CLIENT_ID`,
`NORI_GMAIL_CLIENT_SECRET`); see `.env.example`. Tokens and the mail index
stay in Electron `userData` (`0600`).
