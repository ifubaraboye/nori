// Port of nori-ui model/cache.rs — on-disk mail index beside the account token.
import { mkdirSync, readFileSync, writeFileSync, rmSync, renameSync, chmodSync } from "node:fs";
import { dirname, join } from "node:path";
import { homedir } from "node:os";

export interface CachedEmail {
  id: string;
  sender: string;
  address: string;
  recipients: string[];
  subject: string;
  preview: string;
  timestamp: string;
  fullDate: string;
  mailbox: string;
  unread: boolean;
  starred: boolean;
  threadId?: string;
}

export interface CachedLabel {
  /** Nori's own numeric id, as the Rust index stored it. */
  id: number;
  name: string;
  colour: number;
  /**
   * The Gmail label id behind it, when it came from a connected account.
   * Absent for a label that only ever existed locally, which is why the
   * assignments below cannot be written back without it.
   */
  remoteId?: string;
}

export interface MailIndex {
  account: string;
  emails: CachedEmail[];
  labels: CachedLabel[];
  assignments: Array<[string, number[]]>;
  historyId?: string;
}

export function emptyIndex(): MailIndex {
  return { account: "", emails: [], labels: [], assignments: [] };
}

export function isIndexEmpty(index: MailIndex): boolean {
  return index.emails.length === 0;
}

// Never let a thin store clobber a fuller file (incremental applied to an
// incomplete store, or a save racing a clear).
export function mayReplaceIndex(current: MailIndex, mailCount: number): boolean {
  return current.emails.length <= mailCount;
}

export class IndexCache {
  constructor(readonly path: string) {}

  static withAccount(account: string): IndexCache {
    const base = process.env.XDG_CONFIG_HOME ?? join(homedir(), ".config");
    return new IndexCache(join(base, "nori", `${account}.index.json`));
  }

  load(account: string): MailIndex | null {
    try {
      const contents = readFileSync(this.path, "utf8");
      const index = JSON.parse(contents) as MailIndex;
      return index.account === account ? index : null;
    } catch {
      // Missing or corrupt cache costs one slow sync, not a broken app.
      return null;
    }
  }

  save(index: MailIndex): void {
    const parent = dirname(this.path);
    if (parent) mkdirSync(parent, { recursive: true });
    const temporary = this.path.replace(/\.json$/, ".json.tmp");
    writeFileSync(temporary, JSON.stringify(index));
    renameSync(temporary, this.path);
    try {
      chmodSync(this.path, 0o600);
    } catch { /* non-POSIX */ }
  }

  clear(): void {
    try {
      rmSync(this.path, { force: true });
    } catch { /* idempotent */ }
  }
}

/**
 * Reads from the current location, falling back to the one the Rust build
 * used, and writes only to the current one.
 *
 * Without the fallback the app starts empty on every launch and waits for a
 * slow full sync to rebuild a mailbox it already had on disk. An index found
 * only in the old location is copied forward, so this settles after one run.
 */
export class FallbackIndexCache extends IndexCache {
  constructor(
    path: string,
    private readonly fallback: IndexCache,
  ) {
    super(path);
  }

  override load(account: string): MailIndex | null {
    return super.load(account) ?? this.fallback.load(account);
  }

  override save(index: MailIndex): void {
    super.save(index);
  }
}
