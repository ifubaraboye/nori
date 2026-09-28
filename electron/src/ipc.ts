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
  pinned: boolean;
  /**
   * Always present, always empty in a snapshot.
   *
   * Bodies are fetched the first time a mail is opened, which is what keeps a
   * large mailbox cheap to list. The field still has to exist: the renderer
   * renders `email.body` directly, and a snapshot that omitted it left
   * `undefined.map` to throw, which blanks the reading pane with no message.
   */
  body: string[];
  /** Gmail's own label ids, carried so a label toggle can write back. */
  labelIds: string[];
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
  | "nori:snapshot"
  | "nori:modify"
  | "nori:status"
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
  /** Full account snapshot: every held mail, the cursor, and label counts. */
  snapshot: () => Promise<Snapshot>;
  /** Server-side write-backs. Each returns the labels Gmail settled on. */
  modify: (
    id: string,
    add: string[],
    remove: string[],
  ) => Promise<{ unread: boolean; starred: boolean }>;
  archive: (id: string) => Promise<void>;
  togglePin: (id: string) => Promise<boolean>;
}

export interface Snapshot {
  account: string | null;
  /** True when a first sync is running and mail is still arriving. */
  syncing: boolean;
  emails: Email[];
  historyId?: string;
  labels: Array<{ id: string; name: string; colour: number }>;
  /** Gmail label id per Nori label, so assignments survive a restart. */
  assignments: Array<[string, string[]]>;
}
