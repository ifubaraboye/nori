// Electron main process (Bun runtime) — port of crates/nori-desktop/src/main.rs
// (window shell, menus, app identity) + the nori-gmail sync orchestrator that
// the GPUI host was meant to own. The renderer is web/dist (Vite build).
import { app, BrowserWindow, Menu, ipcMain, shell, dialog } from "electron";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { NORI_PROTOCOL_VERSION, type EmailSummary, type Mailbox } from "./ipc.js";
import { discoverCredentials, FileTokenStore, LastAccount } from "./backend/auth/token.js";
import { beginAuth, awaitCallback, exchange } from "./backend/auth/oauth.js";
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
  labels: Array<{ id: number; name: string; colour: number }>;
  account: string | null;
  historyId?: string;
  settings: ReturnType<typeof defaultSettings>;
}

const state: AppState = {
  window: null,
  emails: new Map(),
  labels: [],
  account: null,
  historyId: undefined,
  settings: defaultSettings(),
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
  const current: MailIndex = {
    account: state.account,
    emails: emails.map((e) => ({ ...e, mailbox: e.mailbox })),
    labels: state.labels,
    assignments: [],
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
    state.labels = index.labels ?? [];
    state.historyId = index.historyId;
  }
}

async function fullSync(): Promise<void> {
  if (!state.account) return;
  const credentials = discoverCredentials();
  const sync = Sync.load(credentials, tokenStoreFor(state.account));
  const snapshot = await sync.full();
  state.account = snapshot.account;
  state.historyId = snapshot.historyId;
  state.emails.clear();
  for (const remote of snapshot.mail) {
    const ui = toUiEmail(remote, snapshot.account);
    state.emails.set(ui.id, ui);
  }
  state.labels = snapshot.labels.map((l, i) => ({ id: i + 1, name: l.name, colour: l.colour }));
  new LastAccount(join(userDataDir(), "last-account")).save(snapshot.account);
  saveIndex();
  emit({ type: "emails-changed" });
}

async function incrementalSync(): Promise<void> {
  if (!state.account || !state.historyId) {
    await fullSync();
    return;
  }
  try {
    const credentials = discoverCredentials();
    const sync = Sync.load(credentials, tokenStoreFor(state.account));
    const result = await sync.incremental(state.historyId);
    if (!result) {
      await fullSync();
      return;
    }
    for (const remote of result.changed) {
      const ui = toUiEmail(remote, state.account);
      state.emails.set(ui.id, ui);
    }
    for (const id of result.deleted) state.emails.delete(id);
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
  ipcMain.handle("nori:open", async (_event, id: string) => {
    const email = state.emails.get(id);
    if (!email) return;
    if (state.settings.markReadOnOpen && email.unread && state.account) {
      email.unread = false;
      try {
        const credentials = discoverCredentials();
        const sync = Sync.load(credentials, tokenStoreFor(state.account));
        await sync.modify(id, [], ["UNREAD"]);
      } catch { /* offline: local state still updated */ }
      saveIndex();
      emit({ type: "emails-changed" });
    }
  });
  ipcMain.handle("nori:toggleStar", async (_event, id: string) => {
    const email = state.emails.get(id);
    if (!email) return false;
    email.starred = !email.starred;
    if (state.account) {
      try {
        const credentials = discoverCredentials();
        const sync = Sync.load(credentials, tokenStoreFor(state.account));
        await sync.modify(id, email.starred ? ["STARRED"] : [], email.starred ? [] : ["STARRED"]);
      } catch { /* offline */ }
      saveIndex();
    }
    emit({ type: "emails-changed" });
    return email.starred;
  });
  ipcMain.handle("nori:search", async (_event, query: string) => {
    const local = [...state.emails.values()].filter((e) => emailMatches(e, query));
    if (!state.account || !query.trim()) return summaries(local);
    try {
      const credentials = discoverCredentials();
      const sync = Sync.load(credentials, tokenStoreFor(state.account));
      const remotes: RemoteMail[] = await sync.fetch(query, 100);
      return summaries(remotes.map((r) => toUiEmail(r, state.account as string)));
    } catch {
      return summaries(local);
    }
  });
  ipcMain.handle("nori:send", async (_event, draft: { to: string; subject: string; body: string }) => {
    if (!state.account) throw new Error("not signed in");
    const credentials = discoverCredentials();
    const sync = Sync.load(credentials, tokenStoreFor(state.account));
    await sync.send(draft.to, draft.subject, draft.body, []);
  });
  ipcMain.handle("nori:fetchBody", async (_event, id: string) => {
    if (!state.account) return [];
    try {
      const credentials = discoverCredentials();
      const sync = Sync.load(credentials, tokenStoreFor(state.account));
      const bodies = await sync.bodies([id]);
      return bodies[0]?.[1] ?? [];
    } catch {
      return [];
    }
  });
  ipcMain.handle("nori:counts", () => {
    const all = [...state.emails.values()];
    const count = (mailbox: Mailbox): number => visibleEmails(mailbox).length;
    void all;
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
    const request = beginAuth(credentials);
    // Fire-and-forget the callback wait; open the browser immediately.
    const codePromise = awaitCallback(request);
    await shell.openExternal(request.url);
    const code = await codePromise;
    const token = await exchange(credentials, request.redirectUri, code, request.verifier);
    // Profile lookup determines the account address for the token filename.
    const { profile } = await import("./backend/gmail/gmail.js");
    const prof = await profile(token.accessToken);
    const store = tokenStoreFor(prof.email || "default");
    store.save(token);
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
    emit({ type: "account-changed", address: null });
  });
  ipcMain.handle("nori:settings:get", () => state.settings);
  ipcMain.handle("nori:settings:set", (_event, settings: typeof state.settings) => {
    state.settings = { ...state.settings, ...settings };
    SettingsStore.withConfigDir().save(state.settings);
    return state.settings;
  });

  // Unused today but reserved for parity with the Rust mailbox fetches.
  void gmailQuery;
  void mailboxOf;
  void bodyParagraphs;
  void plainText;
  void parseHtmlBody;
  void NORI_PROTOCOL_VERSION;
  void homedir;
  void dialog;
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
