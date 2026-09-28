// Typed IPC channels between renderer (web/) and the Bun/Electron main process.
// Extends the window.nori contract sketched in web/src/bridge/noriBridge.ts
// with the auth/sync/labels/counts operations the Rust host was meant to own.
export const NORI_PROTOCOL_VERSION = 1;

export type Mailbox = "inbox" | "starred" | "sent" | "drafts" | "archive" | "trash";

export interface EmailSummary {
  id: string;
  sender: string;
  subject: string;
  preview: string;
  timestamp: string;
  unread: boolean;
  starred: boolean;
}

export interface Email extends EmailSummary {
  address: string;
  recipients: string[];
  fullDate: string;
  mailbox: Mailbox;
  threadId: string;
}

export interface DraftSeed {
  to: string;
  subject: string;
  body: string;
}

export type NoriEvent =
  | { type: "emails-changed" }
  | { type: "mailbox-changed"; mailbox: Mailbox }
  | { type: "account-changed"; address: string | null };

export type NoriInvokeChannel =
  | "nori:list"
  | "nori:get"
  | "nori:open"
  | "nori:toggleStar"
  | "nori:togglePin"
  | "nori:archive"
  | "nori:search"
  | "nori:send"
  | "nori:fetchBody"
  | "nori:labels"
  | "nori:counts"
  | "nori:account"
  | "nori:signin"
  | "nori:signout"
  | "nori:sync"
  | "nori:settings:get"
  | "nori:settings:set";

export interface NoriBridgeApi {
  protocolVersion: number;
  list: (mailbox: Mailbox) => Promise<EmailSummary[]>;
  get: (id: string) => Promise<Email | null>;
  open: (id: string) => Promise<void>;
  toggleStar: (id: string) => Promise<boolean>;
  search: (query: string) => Promise<EmailSummary[]>;
  send: (draft: DraftSeed) => Promise<void>;
  subscribe: (cb: (event: NoriEvent) => void) => () => void;
  // Extended surface (main-process Gmail backend):
  signin: () => Promise<string>;
  signout: () => Promise<void>;
  sync: () => Promise<{ account: string | null }>;
  fetchBody: (id: string) => Promise<string[]>;
  counts: () => Promise<Record<Mailbox, number>>;
}
