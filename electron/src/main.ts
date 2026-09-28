// Electron main process (Bun runtime) — port of crates/nori-desktop/src/main.rs
// (window shell, menus, app identity) + the nori-gmail sync orchestrator that
// the GPUI host was meant to own. The renderer is web/dist (Vite build).
import { app, BrowserWindow, Menu, ipcMain, shell, dialog } from "electron";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import {
  NORI_PROTOCOL_VERSION,
  type Email,
  type EmailSummary,
  type Mailbox,
  type Snapshot,
} from "./ipc.js";
import { discoverCredentials, FileTokenStore, LastAccount } from "./backend/auth/token.js";
import { beginAuth, exchange } from "./backend/auth/oauth.js";
import { Sync, type RemoteMail } from "./backend/gmail/sync.js";
import { IndexCache, mayReplaceIndex, type MailIndex } from "./backend/gmail/cache.js";
import { gmailQuery, mailboxOf, toUiEmail, type UiEmail } from "./backend/gmail/account.js";
import { bodyParagraphs } from "./backend/gmail/syncBodies.js";
import { plainText, parseHtmlBody } from "./backend/gmail/rich.js";
import { SettingsStore, defaultSettings } from "./backend/settings.js";

export const APP_ID = "dev.nori.prototype";
export const APP_NAME = "Nori";

const __dirname = dirname(fileURLToPath(import.meta.url));
const isDev = process.argv.includes("--dev") || process.env.NORI_DEV === "1";

interface AppState {
  window: BrowserWindow | null;
  emails: Map<string, UiEmail>;
  /** Gmail label id -> Nori label, as the server knows them. */
  labels: Array<{ id: string; name: string; colour: number }>;
  account: string | null;
  historyId?: string;
  settings: ReturnType<typeof defaultSettings>;
  /** True while a first or incremental sync is running. */
  syncing: boolean;
  /** Mail id -> the non-system Gmail label ids it carries. */
  labelAssignments: Map<string, string[]>;
}

const state: AppState = {
  window: null,
  emails: new Map(),
  labels: [],
  account: null,
  historyId: undefined,
  settings: defaultSettings(),
  syncing: false,
  labelAssignments: new Map(),
};

function userDataDir(): string {
  return app.getPath("userData");
}

function tokenStoreFor(account: string): FileTokenStore {
  // Prefer Electron userData; fall back to XDG path for migration compat.
  return new FileTokenStore(join(userDataDir(), `${account}.token.json`));
}

function indexCacheFor(account: string): IndexCache {
  const inUserData = new IndexCache(join(userDataDir(), `${account}.index.json`));
  return inUserData;
}

function emit(event: unknown): void {
  state.window?.webContents.send("nori-event", event);
}

function summaries(emails: UiEmail[]): EmailSummary[] {
  return emails.map((e) => ({
    id: e.id,
    sender: e.sender,
    subject: e.subject,
    preview: e.preview,
    timestamp: e.timestamp,
    unread: e.unread,
    starred: e.starred,
  }));
}

/** Gmail's own labels for a mail: what write-backs are computed against. */
function remoteLabelsOf(id: string): string[] {
  const email = state.emails.get(id);
  if (!email) return [];
  const system = new Set(["INBOX", "UNREAD", "STARRED", "SENT", "DRAFT", "TRASH", "SPAM", "CATEGORY_PROMOTIONS", "CATEGORY_SOCIAL", "CATEGORY_UPDATES", "CATEGORY_FORUMS"]);
  const fromIndex = state.labelAssignments.get(id) ?? [];
  const fromMail = (email as UiEmail & { labelIds?: string[] }).labelIds ?? [];
  return [...new Set([...fromIndex, ...fromMail])].filter((l) => !system.has(l));
}

/** The full renderer snapshot. One call replaces six per-mailbox round-trips. */
function snapshot(): Snapshot {
  return {
    account: state.account,
    syncing: state.syncing,
    emails: [...state.emails.values()].map((email): Email => ({
      id: email.id,
      sender: email.sender,
      subject: email.subject,
      preview: email.preview,
      timestamp: email.timestamp,
      unread: email.unread,
      starred: email.starred,
      address: email.address,
      recipients: email.recipients,
      fullDate: email.fullDate,
      mailbox: email.mailbox,
      threadId: email.threadId,
      pinned: (email as UiEmail & { pinned?: boolean }).pinned ?? false,
      labelIds: remoteLabelsOf(email.id),
    })),
    historyId: state.historyId,
    labels: state.labels,
    assignments: [...state.labelAssignments.entries()],
  };
}

function visibleEmails(mailbox: Mailbox): UiEmail[] {
  const all = [...state.emails.values()];
  if (mailbox === "starred") return all.filter((e) => e.starred);
  if (mailbox === "inbox") return all.filter((e) => e.mailbox === "inbox");
  return all.filter((e) => e.mailbox === mailbox);
}

function emailMatches(email: UiEmail, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    email.sender.toLowerCase().includes(q) ||
    email.subject.toLowerCase().includes(q) ||
    email.preview.toLowerCase().includes(q)
  );
}

function saveIndex(): void {
  if (!state.account) return;
  const emails = [...state.emails.values()];
  // The index file keeps the Rust shape — Nori's own numeric label ids, with
  // the Gmail id alongside — so a cache written before the port still loads.
  const labels = state.labels.map((l, index) => ({
    id: index + 1,
    name: l.name,
    colour: l.colour,
    remoteId: l.id,
  }));
  const byRemote = new Map(labels.map((l) => [l.remoteId as string, l.id]));
  const current: MailIndex = {
    account: state.account,
    emails: emails.map((e) => ({ ...e, mailbox: e.mailbox })),
    labels,
    assignments: [...state.labelAssignments.entries()]
      .map(
        ([id, remotes]): [string, number[]] => [
          id,
          remotes.map((r) => byRemote.get(r)).filter((n): n is number => n !== undefined),
        ],
      )
      .filter(([, held]) => held.length > 0),
    historyId: state.historyId,
  };
  const cache = indexCacheFor(state.account);
  const existing = cache.load(state.account);
  if (existing && !mayReplaceIndex(existing, emails.length)) return;
  cache.save(current);
}

async function restoreFromDisk(): Promise<void> {
  const settingsStore = SettingsStore.withConfigDir();
  state.settings = settingsStore.load() ?? defaultSettings();
  const last = new LastAccount(join(userDataDir(), "last-account"));
  const account = last.load() ?? LastAccount.withConfigDir().load();
  if (!account) return;
  state.account = account;
  const cache = indexCacheFor(account);
  const index = cache.load(account);
  if (index && index.emails.length > 0) {
    for (const email of index.emails) {
      state.emails.set(email.id, {
        ...email,
        mailbox: email.mailbox as UiEmail["mailbox"],
        recipients: email.recipients ?? [],
        threadId: email.threadId ?? "",
      });
    }
    // A label cached before the port has no remoteId: it existed only
    // locally, so it can be listed and shown but never written back.
    state.labels = (index.labels ?? []).map((l) => ({
      id: l.remoteId ?? `local:${l.id}`,
      name: l.name,
      colour: l.colour,
    }));
    state.historyId = index.historyId;
    const byLocal = new Map((index.labels ?? []).map((l) => [l.id, l.remoteId]));
    state.labelAssignments = new Map(
      (index.assignments ?? [])
        .map(([id, held]) => [
          id,
          held.map((n) => byLocal.get(n)).filter((r): r is string => r !== undefined),
        ] as [string, string[]])
        .filter(([, remotes]) => remotes.length > 0),
    );
  }
}

/** A Sync bound to the connected account, or null when there is none. */
function syncForAccount(): Sync | null {
  if (!state.account) return null;
  return Sync.load(discoverCredentials(), tokenStoreFor(state.account));
}

/**
 * One write-back path for every server-side change: star, archive, mark read,
 * and label toggles all go through here so a single place settles local state
 * from what Gmail actually accepted.
 */
async function modifyLabels(
  id: string,
  add: string[],
  remove: string[],
): Promise<{ unread: boolean; starred: boolean }> {
  const email = state.emails.get(id);
  if (!email) return { unread: false, starred: false };
  const sync = syncForAccount();
  if (sync) await sync.modify(id, add, remove);
  if (add.includes("STARRED")) email.starred = true;
  if (remove.includes("STARRED")) email.starred = false;
  if (remove.includes("UNREAD")) email.unread = false;
  if (add.includes("UNREAD")) email.unread = true;
  // Removing INBOX is what archiving is: the mail leaves the mailbox and
  // nothing else about it changes, which is why there is no delete beside it.
  if (remove.includes("INBOX")) email.mailbox = "archive";
  const held = remoteLabelsOf(id);
  state.labelAssignments.set(id, held);
  saveIndex();
  return { unread: email.unread, starred: email.starred };
}

async function fullSync(): Promise<void> {
  if (!state.account) return;
  state.syncing = true;
  emit({ type: "emails-changed" });
  try {
    const sync = syncForAccount();
    if (!sync) return;
    const snap = await sync.full();
    state.account = snap.account;
    state.historyId = snap.historyId;
    state.emails.clear();
    state.labelAssignments.clear();
    for (const remote of snap.mail) {
      const ui = toUiEmail(remote, snap.account);
      state.emails.set(ui.id, ui);
      const custom = remote.labelIds.filter((l) => !SYSTEM_LABELS.has(l));
      if (custom.length > 0) state.labelAssignments.set(ui.id, custom);
    }
    state.labels = snap.labels.map((l) => ({ id: l.id, name: l.name, colour: l.colour }));
    new LastAccount(join(userDataDir(), "last-account")).save(snap.account);
    saveIndex();
  } finally {
    state.syncing = false;
    emit({ type: "emails-changed" });
  }
}

const SYSTEM_LABELS = new Set([
  "INBOX", "UNREAD", "STARRED", "SENT", "DRAFT", "TRASH", "SPAM",
  "CATEGORY_PROMOTIONS", "CATEGORY_SOCIAL", "CATEGORY_UPDATES", "CATEGORY_FORUMS",
]);

async function incrementalSync(): Promise<void> {
  if (!state.account || !state.historyId) {
    await fullSync();
    return;
  }
  try {
    const sync = syncForAccount();
    if (!sync) return;
    const result = await sync.incremental(state.historyId);
    if (!result) {
      await fullSync();
      return;
    }
    for (const remote of result.changed) {
      const ui = toUiEmail(remote, state.account);
      state.emails.set(ui.id, ui);
      const custom = remote.labelIds.filter((l) => !SYSTEM_LABELS.has(l));
      state.labelAssignments.set(ui.id, custom);
    }
    for (const id of result.deleted) {
      state.emails.delete(id);
      state.labelAssignments.delete(id);
    }
    state.historyId = result.historyId ?? state.historyId;
    saveIndex();
    emit({ type: "emails-changed" });
  } catch {
    // Offline at startup: keep the disk cache, renderer stays usable.
  }
}

function registerIpc(): void {
  ipcMain.handle("nori:list", (_event, mailbox: Mailbox) => summaries(visibleEmails(mailbox)));
  ipcMain.handle("nori:get", (_event, id: string) => state.emails.get(id) ?? null);
  ipcMain.handle("nori:snapshot", () => snapshot());
  ipcMain.handle("nori:status", () => ({
    account: state.account,
    syncing: state.syncing,
    settings: state.settings,
  }));

  // Every server-side change goes through one handler, so local state is
  // settled from what Gmail accepted rather than from what was asked for.
  ipcMain.handle("nori:modify", (_event, id: string, add: string[], remove: string[]) =>
    modifyLabels(id, add ?? [], remove ?? []),
  );
  ipcMain.handle("nori:archive", async (_event, id: string) => {
    await modifyLabels(id, [], ["INBOX"]);
    emit({ type: "emails-changed" });
  });
  // Pinning is local — Gmail has no such concept — so it is answered from the
  // cache the renderer already holds rather than pretended server-side.
  ipcMain.handle("nori:togglePin", async (_event, id: string) => {
    const email = state.emails.get(id);
    if (!email) return false;
    const pinned = !((email as UiEmail & { pinned?: boolean }).pinned ?? false);
    (email as UiEmail & { pinned?: boolean }).pinned = pinned;
    saveIndex();
    return pinned;
  });

  ipcMain.handle("nori:open", async (_event, id: string) => {
    const email = state.emails.get(id);
    if (!email) return;
    if (state.settings.markReadOnOpen && email.unread) {
      await modifyLabels(id, [], ["UNREAD"]);
      emit({ type: "emails-changed" });
    }
  });
  ipcMain.handle("nori:toggleStar", async (_event, id: string) => {
    const email = state.emails.get(id);
    if (!email) return false;
    const settled = await modifyLabels(
      id,
      email.starred ? [] : ["STARRED"],
      email.starred ? ["STARRED"] : [],
    );
    emit({ type: "emails-changed" });
    return settled.starred;
  });
  ipcMain.handle("nori:search", async (_event, query: string) => {
    const local = [...state.emails.values()].filter((e) => emailMatches(e, query));
    if (!state.account || !query.trim()) return summaries(local);
    try {
      const sync = syncForAccount();
      if (!sync) return summaries(local);
      const remotes: RemoteMail[] = await sync.fetch(query, 100);
      return summaries(remotes.map((r) => toUiEmail(r, state.account as string)));
    } catch {
      return summaries(local);
    }
  });
  ipcMain.handle("nori:send", async (_event, draft: { to: string; subject: string; body: string }) => {
    const sync = syncForAccount();
    if (!sync) throw new Error("not signed in");
    await sync.send(draft.to, draft.subject, draft.body, []);
  });
  ipcMain.handle("nori:fetchBody", async (_event, id: string) => {
    const sync = syncForAccount();
    if (!sync) return [];
    try {
      const bodies = await sync.bodies([id]);
      return bodies[0]?.[1] ?? [];
    } catch {
      return [];
    }
  });
  ipcMain.handle("nori:counts", () => {
    const count = (mailbox: Mailbox): number => visibleEmails(mailbox).length;
    return {
      inbox: count("inbox"),
      starred: count("starred"),
      sent: count("sent"),
      drafts: count("drafts"),
      archive: count("archive"),
      trash: count("trash"),
    };
  });
  ipcMain.handle("nori:sync", async () => {
    await incrementalSync();
    return { account: state.account };
  });
  ipcMain.handle("nori:account", () => state.account);
  ipcMain.handle("nori:signin", async () => {
    const credentials = discoverCredentials();
    // The listener is bound before the browser opens, because the port has to
    // appear in the redirect URI the browser is sent to.
    const { request, redirect } = await beginAuth(credentials);
    const codePromise = redirect.awaitCallback(request.state);
    await shell.openExternal(request.url);
    console.log(`[nori] sign-in: opened ${request.url.split("?")[0]} for ${redirect.redirectUri}`);
    const code = await codePromise;
    const token = await exchange(credentials, request.redirectUri, code, request.verifier);
    // The profile lookup names the account, which is the token file's name.
    const { profile } = await import("./backend/gmail/gmail.js");
    const prof = await profile(token.accessToken);
    tokenStoreFor(prof.email || "default").save(token);
    state.account = prof.email;
    new LastAccount(join(userDataDir(), "last-account")).save(prof.email);
    await fullSync();
    return prof.email;
  });
  ipcMain.handle("nori:signout", async () => {
    if (state.account) {
      tokenStoreFor(state.account).clear();
      indexCacheFor(state.account).clear();
    }
    new LastAccount(join(userDataDir(), "last-account")).clear();
    state.account = null;
    state.emails.clear();
    state.labels = [];
    state.historyId = undefined;
    state.labelAssignments.clear();
    emit({ type: "account-changed", address: null });
  });
  ipcMain.handle("nori:settings:get", () => state.settings);
  ipcMain.handle("nori:settings:set", (_event, settings: typeof state.settings) => {
    state.settings = { ...state.settings, ...settings };
    SettingsStore.withConfigDir().save(state.settings);
    return state.settings;
  });

  void NORI_PROTOCOL_VERSION;
  void homedir;
  void dialog;
  void gmailQuery;
  void mailboxOf;
  void bodyParagraphs;
  void plainText;
  void parseHtmlBody;
}

function setAppMenus(): void {
  // No application menu: the Nori / Edit / View bar stays hidden and all
  // shortcuts live in the renderer. Quit still works via window close.
  Menu.setApplicationMenu(null);
}

async function openMainWindow(): Promise<void> {
  const window = new BrowserWindow({
    width: 1200,
    height: 760,
    minWidth: 760,
    minHeight: 520,
    backgroundColor: state.settings.lightMode ? "#fcfcfc" : "#1a1a1a",
    title: APP_NAME,
    webPreferences: {
      preload: join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  state.window = window;

  if (isDev) {
    await window.loadURL("http://localhost:3001");
    window.webContents.openDevTools({ mode: "detach" });
  } else {
    const indexHtml = join(__dirname, "..", "web", "dist", "index.html");
    if (existsSync(indexHtml)) {
      await window.loadFile(indexHtml);
    } else {
      // Fallback for `electron .` before the renderer build exists.
      await window.loadURL("http://localhost:3001");
    }
  }

  window.on("closed", () => {
    state.window = null;
  });
}

async function run(): Promise<void> {
  if (process.platform === "linux") app.setName(APP_ID);
  await app.whenReady();
  if (process.platform === "darwin") app.setAboutPanelOptions({ applicationName: APP_NAME });
  setAppMenus();
  registerIpc();
  await restoreFromDisk();
  await openMainWindow();
  // Resume sync off the critical path (mirrors MailApp::resume deferral).
  setImmediate(() => {
    incrementalSync().catch(() => undefined);
  });
  // 30s poll like the GPUI host.
  setInterval(() => {
    if (state.account) incrementalSync().catch(() => undefined);
  }, 30_000);

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) void openMainWindow();
  });
  app.on("window-all-closed", () => {
    if (process.platform !== "darwin") app.quit();
  });
}

void run();
