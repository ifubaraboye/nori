/**
 * Future bridge contract for the Rust/GPUI host.
 *
 * The web UI currently runs standalone against the in-memory store in
 * `src/state/store.ts` and MUST NOT call into this bridge yet.
 *
 * When the GPUI host embeds the built `web/dist/` output (Waku `src/browser.rs`
 * pattern: WKWebView on macOS / composition WebView2 on Windows, geometry +
 * visibility sync owned by Rust), it can expose this surface before loading
 * the page. The web app will then switch its store adapter from mock data to
 * these calls without changing components.
 *
 * Two shapes are reserved (pick one when the host lands):
 *
 * 1. `window.nori` RPC (preferred for an embedded webview, mirrors the
 *    Waku `waku-protocol` + generated `waku-client` split: versioned JSON,
 *    request ids, subscriptions):
 *
 *    window.nori = {
 *      protocolVersion: 1,
 *      list(mailbox): Promise<EmailSummary[]>,
 *      get(id): Promise<Email | null>,
 *      open(id): Promise<void>,        // marks read, manages tabs server-side
 *      toggleStar(id): Promise<boolean>,
 *      search(query): Promise<EmailSummary[]>,
 *      send(draft): Promise<void>,
 *      subscribe(cb): () => void,      // pushes { type: "emails-changed" }
 *    }
 *
 * 2. `postMessage` fallback for hosts that prefer message ports over direct
 *    injection. Same payloads, `{ channel: "nori", id, method, params }`
 *    requests and `{ channel: "nori", id, ok, result }` responses, plus
 *    `{ channel: "nori-event", event }` pushes.
 *
 * Security notes (from Waku `apps/web`): the token/capability, if any, stays
 * between browser and daemon; the static file server never sees it.
 */

import type { DraftSeed, Email, EmailId, EmailSummary, Mailbox } from "../types/mail";
import type { SettingsState } from "../state/settings";

export const NORI_PROTOCOL_VERSION = 1;

export interface NoriBridge {
  protocolVersion: number;
  // Host ids are Gmail strings; the local mock store uses numbers. Both flow
  // through here, so every id-typed method accepts either.
  list: (mailbox: Mailbox) => Promise<EmailSummary[]>;
  get: (id: EmailId | string) => Promise<Email | null>;
  open: (id: EmailId | string) => Promise<void>;
  toggleStar: (id: EmailId | string) => Promise<boolean>;
  search: (query: string) => Promise<EmailSummary[]>;
  send: (draft: DraftSeed) => Promise<void>;
  subscribe: (cb: (event: NoriEvent) => void) => () => void;
  // Extended Electron host surface (optional; present under Electron,
  // absent in standalone web). String ids because Gmail ids are strings.
  sync?: () => Promise<{ account: string | null }>;
  counts?: () => Promise<Record<Mailbox, number>>;
  fetchBody?: (id: string) => Promise<string[]>;
  signin?: () => Promise<string>;
  signout?: () => Promise<void>;
  settingsGet?: () => Promise<Partial<SettingsState> | null>;
  settingsSet?: (settings: SettingsState) => Promise<SettingsState>;
}

export type NoriEvent =
  | { type: "emails-changed" }
  | { type: "mailbox-changed"; mailbox: Mailbox }
  | { type: "account-changed"; address: string | null }
  | { type: "toggle-sidebar" };

export type NoriRequest =
  | { channel: "nori"; id: number; method: "list"; params: { mailbox: Mailbox } }
  | { channel: "nori"; id: number; method: "get"; params: { id: EmailId } }
  | { channel: "nori"; id: number; method: "open"; params: { id: EmailId } }
  | { channel: "nori"; id: number; method: "toggleStar"; params: { id: EmailId } }
  | { channel: "nori"; id: number; method: "search"; params: { query: string } }
  | { channel: "nori"; id: number; method: "send"; params: { draft: DraftSeed } };

declare global {
  interface Window {
    nori?: NoriBridge;
  }
}

/** Returns the injected host bridge when present, otherwise null (standalone mode). */
export function getNoriBridge(): NoriBridge | null {
  if (typeof window === "undefined") return null;
  const bridge = window.nori;
  if (bridge && bridge.protocolVersion === NORI_PROTOCOL_VERSION) return bridge;
  return null;
}
