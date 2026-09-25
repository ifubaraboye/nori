# Nori Web UI

Standalone React + TypeScript email UI. It mirrors the existing GPUI prototype
(`crates/nori-ui`) pixel-for-pixel and runs entirely on mock data. No Rust or
GPUI changes are needed to develop it.

## Run

```
cd web
bun install
bun run dev      # http://localhost:3001
bun run typecheck
bun run build    # emits dist/ for the future GPUI host to embed
bun run preview
```

## Layout

- `src/types/mail.ts` — port of `model/mail.rs` types + `Email::matches`.
- `src/data/mock.ts` — port of `model/mock.rs` (18 emails).
- `src/state/store.ts` — port of `MailStore` as a React reducer
  (`select-mailbox`, `open-email`, `close-tab`, `cycle-tab`, `move-selection`,
  `toggle-star`, overlays, `replySeed` from `mail_app.rs`).
- `src/theme/tokens.css` — port of `Theme::dark()` in `theme.rs` as CSS variables.
- `src/components/` — one TSX + CSS module per Rust view:
  `Sidebar`, `EmailList` (inbox + row), `EmailTabs`, `EmailView`,
  `ComposeDialog` (620x492), `SearchDialog` (680x430), `Button`, `Icon`, `MailApp`.
- `src/bridge/noriBridge.ts` — **contract only**. Defines the future
  `window.nori` RPC + `postMessage` shapes the Rust host will expose
  (versioned, request ids, subscriptions — same split as Waku's
  `waku-protocol` + generated `waku-client`). Nothing calls it yet.
- `src/assets/icons/` + `public/icons/` — copies of
  `crates/nori-desktop/assets/icons/*.svg` (they use `currentColor`, so `Icon`
  inlines the raw SVG to preserve tint).

## Keyboard map (mirrors `actions.rs`)

`j` / `k` move, `Enter` opens, `c` composes, `/` searches, `Esc` dismisses /
goes back, `Ctrl/Cmd-W` closes the active tab, `Ctrl/Cmd-Tab` (+Shift) cycles
tabs, `Ctrl/Cmd-B` toggles the sidebar. Search handles `Up/Down/Enter/Esc`
itself. Sidebar resize handle supports `Left/Right` (±8, ±20 with Shift),
`Home`/`End`.

## Future host integration (not implemented)

Build with `bun run build`, then have the GPUI shell load `dist/index.html`
in a `browser.rs`-style webview (WKWebView / WebView2 composition, Rust owns
geometry + visibility + focus sync). In dev, point the webview at
`http://localhost:3001`. When ready, implement `window.nori` per
`src/bridge/noriBridge.ts` and swap the store adapter; components stay the same.
