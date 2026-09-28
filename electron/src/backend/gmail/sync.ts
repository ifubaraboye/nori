// Port of crates/nori-gmail/src/sync.rs — Gmail -> RemoteMail sync orchestrator.
import { GmailHttpError } from "./http.js";
import {
  profile, labels, label, listMessages, messageMetadata, messageFull,
  history, attachment as fetchAttachment, modifyLabels, sendMessage,
  isSystemLabel, isVisibleLabel, headerOf, unixSecondsOf,
  type MessageMetadata,
} from "./gmail.js";
import { folderCountsFromLookup, type FolderCounts } from "./counts.js";
import { bodyParagraphs, attachmentsAndBlocks } from "./syncBodies.js";
import { parseRecipients, buildSendRaw, type SendAttachment } from "./send.js";
import { refresh, type Credentials } from "../auth/oauth.js";
import { isFresh, type Token, type TokenStore } from "../auth/token.js";
import type { MailStream } from "./stream.js";

export const METADATA_BUDGET = 200;
export const MAILBOX_FETCH_BUDGET = 100;
export const SEARCH_BUDGET = 100;
export const INBOX_QUERY = "in:inbox";
export const ANYWHERE_QUERY = "in:anywhere";
const FETCH_CONCURRENCY = 4;
const MAX_PAGES = 10;
const QUOTA_UNITS_PER_MINUTE = 6000;
const SELF_LIMIT_UNITS_PER_MINUTE = Math.floor((QUOTA_UNITS_PER_MINUTE * 4) / 5);
const COST_MESSAGE_GET = 20;
const COST_MESSAGE_LIST = 5;

export interface RemoteLabel {
  id: string;
  name: string;
  colour: number;
}

export interface RemoteMail {
  id: string;
  sender: string;
  address: string;
  recipients: string[];
  subject: string;
  preview: string;
  threadId: string;
  labelIds: string[];
  timestamp: string;
  fullDate: string;
}

export function hasLabel(mail: RemoteMail, label: string): boolean {
  return mail.labelIds.includes(label);
}

export interface Snapshot {
  account: string;
  labels: RemoteLabel[];
  mail: RemoteMail[];
  historyId?: string;
}

export interface Incremental {
  changed: RemoteMail[];
  deleted: string[];
  historyId?: string;
}

export interface Delta {
  changed: string[];
  deleted: string[];
  historyId?: string;
}

export function isDeltaEmpty(delta: Delta): boolean {
  return delta.changed.length === 0 && delta.deleted.length === 0;
}

export type SyncOutcome = { kind: "full"; snapshot: Snapshot } | { kind: "changes"; incremental: Incremental };

export interface AttachmentMeta {
  filename: string;
  mimeType: string;
  size: number;
  attachmentId: string;
}

class RateLimiter {
  private next = 0;
  constructor(readonly unitsPerMinute: number) {}

  async acquire(units: number): Promise<void> {
    const spacing = 60_000 / this.unitsPerMinute;
    const now = Date.now();
    const grant = Math.max(this.next, now);
    this.next = grant + spacing * units;
    const wait = grant - now;
    if (wait > 0) await new Promise((resolve) => setTimeout(resolve, wait));
  }
}

export class Sync {
  private token: Token;
  private readonly limiter = new RateLimiter(SELF_LIMIT_UNITS_PER_MINUTE);

  constructor(
    private readonly credentials: Credentials,
    private readonly store: TokenStore,
    token: Token,
  ) {
    this.token = token;
  }

  static load(credentials: Credentials, store: TokenStore): Sync {
    const token = store.load();
    if (!token) throw new Error("not signed in");
    return new Sync(credentials, store, token);
  }

  currentToken(): Token {
    return this.token;
  }

  async authorize(): Promise<void> {
    if (isFresh(this.token)) return;
    if (!this.token.refreshToken) throw new Error("not signed in");
    const renewed = await refresh(this.credentials, this.token.refreshToken);
    this.token = renewed;
    this.store.save(renewed);
  }

  async full(stream?: MailStream): Promise<Snapshot> {
    await this.authorize();
    const prof = await profile(this.token.accessToken);
    const remoteLabels = await this.remoteLabels();
    const mail = await this.fetchWith(INBOX_QUERY, METADATA_BUDGET, new Set(), stream);
    return { account: prof.email, labels: remoteLabels, mail, historyId: prof.historyId };
  }

  async fetch(query: string, budget: number): Promise<RemoteMail[]> {
    return this.fetchWith(query, budget, new Set(), undefined);
  }

  async fetchWith(
    query: string,
    budget: number,
    held: Set<string>,
    stream?: MailStream,
  ): Promise<RemoteMail[]> {
    await this.authorize();
    const ids = await this.listIds(query, budget, held);
    const metas = await this.metadataFor(ids, stream);
    const out: RemoteMail[] = [];
    for (const meta of metas) {
      if (!meta) continue;
      const mail = toMail(meta);
      if (mail) out.push(mail);
    }
    stream?.finish();
    return out;
  }

  private async listIds(query: string, budget: number, held: Set<string>): Promise<string[]> {
    const ids: string[] = [];
    let pageToken: string | undefined;
    for (let page = 0; page < MAX_PAGES && ids.length < budget; page++) {
      await this.limiter.acquire(COST_MESSAGE_LIST);
      const list = await listMessages(this.token.accessToken, query, pageToken);
      for (const message of list.messages) {
        if (held.has(message.id)) continue;
        ids.push(message.id);
        if (ids.length >= budget) break;
      }
      pageToken = list.nextPage;
      if (!pageToken) break;
    }
    return ids;
  }

  async incremental(since: string): Promise<Incremental | null> {
    const delta = await this.delta(since);
    if (!delta) return null;
    const metas = await this.metadataFor(delta.changed, undefined);
    const changed: RemoteMail[] = [];
    for (const meta of metas) {
      if (!meta) continue;
      const mail = toMail(meta);
      if (mail) changed.push(mail);
    }
    return { changed, deleted: delta.deleted, historyId: delta.historyId };
  }

  async delta(since: string): Promise<Delta | null> {
    await this.authorize();
    try {
      const page = await history(this.token.accessToken, since);
      const changed = new Set<string>();
      const deleted = new Set<string>();
      for (const entry of page.entries) {
        for (const m of entry.added) changed.add(m.id);
        for (const m of entry.labels) changed.add(m.id);
        for (const m of entry.deleted) deleted.add(m.id);
      }
      for (const id of deleted) changed.delete(id);
      return {
        changed: [...changed],
        deleted: [...deleted],
        historyId: page.historyId,
      };
    } catch (err) {
      if (err instanceof GmailHttpError && err.error.kind === "api" && err.error.status === 404) {
        return null;
      }
      throw err;
    }
  }

  async bodies(ids: string[]): Promise<Array<[string, string[], AttachmentMeta[]]>> {
    const out: Array<[string, string[], AttachmentMeta[]]> = [];
    for (const id of ids) {
      await this.authorize();
      const full = await messageFull(this.token.accessToken, id);
      const { attachments } = attachmentsAndBlocks(full);
      out.push([id, bodyParagraphs(full), attachments]);
    }
    return out;
  }

  async attachmentBytes(messageId: string, attachmentId: string): Promise<Uint8Array> {
    await this.authorize();
    return fetchAttachment(this.token.accessToken, messageId, attachmentId);
  }

  async modify(id: string, add: string[], remove: string[]): Promise<void> {
    await this.authorize();
    await modifyLabels(this.token.accessToken, id, add, remove);
  }

  async send(to: string, subject: string, body: string, attachments: SendAttachment[]): Promise<void> {
    await this.authorize();
    const recipients = parseRecipients(to);
    const raw = buildSendRaw(recipients, subject, body, attachments);
    await sendMessage(this.token.accessToken, raw);
  }

  async folderCounts(): Promise<FolderCounts> {
    await this.authorize();
    const prof = await profile(this.token.accessToken);
    const cache = new Map<string, [number, number]>();
    const lookup = (id: string): [number, number] | undefined => {
      const cached = cache.get(id);
      if (cached) return cached;
      return undefined;
    };
    for (const id of ["INBOX", "SENT", "DRAFT", "TRASH", "SPAM", "STARRED"]) {
      try {
        const info = await label(this.token.accessToken, id);
        cache.set(id, [info.messagesTotal ?? 0, info.messagesUnread ?? 0]);
      } catch { /* label missing: treat as zero */ }
    }
    return folderCountsFromLookup(lookup, prof.total);
  }

  private async remoteLabels(): Promise<RemoteLabel[]> {
    const all = await labels(this.token.accessToken);
    return all
      .filter((l) => !isSystemLabel(l) && isVisibleLabel(l))
      .map((l) => ({
        id: l.id,
        name: l.name,
        colour: gmailColour(l.color) ?? colourOf(l.name),
      }));
  }

  private async metadataFor(ids: string[], stream?: MailStream): Promise<Array<MessageMetadata | null>> {
    const results: Array<MessageMetadata | null> = new Array(ids.length).fill(null);
    let next = 0;
    let failed: unknown = null;
    const workers: Promise<void>[] = [];
    for (let w = 0; w < Math.min(FETCH_CONCURRENCY, Math.max(1, ids.length)); w++) {
      workers.push(
        (async () => {
          while (failed === null) {
            const index = next++;
            if (index >= ids.length) break;
            await this.limiter.acquire(COST_MESSAGE_GET);
            try {
              const meta = await messageMetadata(this.token.accessToken, ids[index]);
              results[index] = meta;
              if (stream) {
                const mail = toMail(meta);
                if (mail) stream.push(mail);
              }
            } catch (err) {
              if (err instanceof GmailHttpError && err.error.kind === "api" && err.error.status === 404) {
                results[index] = null;
              } else {
                failed = err;
                break;
              }
            }
          }
        })(),
      );
    }
    await Promise.all(workers);
    if (failed !== null) throw failed;
    return results;
  }
}

export function toMail(metadata: MessageMetadata): RemoteMail | null {
  if (!metadata.id) return null;
  const [sender, address] = splitAddress(headerOf(metadata, "From") ?? "");
  const toHeader = headerOf(metadata, "To") ?? "";
  const recipients = toHeader.split(",").map((r) => r.trim()).filter(Boolean);
  const subject = headerOf(metadata, "Subject") || "(no subject)";
  const unix = unixSecondsOf(metadata);
  return {
    id: metadata.id,
    sender: sender || address,
    address,
    recipients,
    subject,
    preview: metadata.snippet ?? "",
    threadId: metadata.threadId ?? "",
    labelIds: metadata.labelIds ?? [],
    timestamp: unix != null ? shortDate(unix) : "",
    fullDate: unix != null ? fullDate(unix) : "Unknown date",
  };
}

function splitAddress(from: string): [string, string] {
  const trimmed = from.trim();
  if (!trimmed) return ["", ""];
  const open = trimmed.lastIndexOf("<");
  const close = trimmed.lastIndexOf(">");
  if (open !== -1 && close > open) {
    const name = trimmed.slice(0, open).trim().replace(/^"+|"+$/g, "");
    return [name, trimmed.slice(open + 1, close).trim()];
  }
  return ["", trimmed];
}

const PALETTE = [0xe2658a, 0xe0a85b, 0x62c987, 0x5bc7b5, 0x62a8e2, 0xa08ce0, 0xd07ac8, 0x9aa5b5];

export function colourOf(name: string): number {
  let hash = 0x811c9dc5;
  for (const byte of Buffer.from(name, "utf8")) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193);
  }
  return PALETTE[Math.abs(hash) % PALETTE.length];
}

function gmailColour(id: string | undefined): number | undefined {
  if (!id) return undefined;
  const match = /(\d+)/.exec(id);
  if (!match) return undefined;
  const table: Record<string, number> = {
    "1": 0xdd6b5b, "2": 0xe89c3c, "3": 0x8a9a5b, "4": 0x5b8a9a,
    "5": 0x9a6b5b, "6": 0x9a5b8a, "7": 0x6b5b9a, "8": 0x5b9a8a,
  };
  return table[match[1]];
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function parts(unix: number): { year: number; month: number; day: number; hour: number; minute: number } {
  const date = new Date(unix * 1000);
  return {
    year: date.getUTCFullYear(),
    month: date.getUTCMonth() + 1,
    day: date.getUTCDate(),
    hour: date.getUTCHours(),
    minute: date.getUTCMinutes(),
  };
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

function shortDate(unix: number): string {
  const p = parts(unix);
  const thisYear = new Date().getUTCFullYear();
  if (p.year === thisYear) return `${p.day} ${MONTHS[p.month - 1]} ${pad(p.hour)}:${pad(p.minute)}`;
  return `${p.day} ${MONTHS[p.month - 1]} ${p.year}`;
}

function fullDate(unix: number): string {
  const p = parts(unix);
  return `${p.day} ${MONTHS[p.month - 1]} ${p.year} ${pad(p.hour)}:${pad(p.minute)}`;
}

export function baseUrl(): string {
  return "https://gmail.googleapis.com/gmail/v1";
}

export function metadataBudget(): number {
  return METADATA_BUDGET;
}
