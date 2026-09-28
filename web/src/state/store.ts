// Port of MailStore in crates/nori-ui/src/model/mail.rs:109-289.
// Pure helpers + a React reducer-based store. No backend calls.
import { useMemo, useReducer, type Dispatch } from "react";
import { mockEmails } from "../data/mock";
import {
  emailSummary,
  type DraftSeed,
  type Email,
  type EmailId,
  type EmailSummary,
  type Mailbox,
  type Overlay,
  type WorkspaceView,
} from "../types/mail";
import type { SettingsPage } from "./settings";

export interface MailState {
  emails: Email[];
  selectedMailbox: Mailbox;
  selectedIndex: number;
  tabs: EmailId[];
  activeTab: EmailId | null;
  workspaceView: WorkspaceView;
  overlay: Overlay | null;
  composeSeed: DraftSeed | null;
  /** Open settings page, or null. Holds the workspace while open. */
  settingsPage: SettingsPage | null;
  /** Visited views, for the sidebar back/forward arrows (mail.rs NavEntry). */
  historyBack: NavEntry[];
  historyForward: NavEntry[];
}

export interface NavEntry {
  view: WorkspaceView;
  mailbox: Mailbox;
  selectedIndex: number;
}

export type MailAction =
  | { type: "select-mailbox"; mailbox: Mailbox }
  | { type: "move-selection"; delta: number }
  | { type: "set-selected-index"; index: number }
  | { type: "open-email"; id: EmailId; markRead?: boolean; openInTab?: boolean }
  | { type: "close-tab"; id: EmailId }
  | { type: "close-active-tab" }
  | { type: "cycle-tab"; direction: number }
  | { type: "toggle-star"; id: EmailId }
  | { type: "open-search" }
  | { type: "open-compose"; seed: DraftSeed }
  | { type: "close-overlay" }
  | { type: "close-compose" }
  | { type: "park-compose"; seed: DraftSeed }
  | { type: "open-email-from-search"; id: EmailId; markRead?: boolean; openInTab?: boolean }
  | { type: "open-settings" }
  | { type: "close-settings" }
  | { type: "set-settings-page"; page: SettingsPage }
  | { type: "go-back" }
  | { type: "go-forward" };

export function visibleEmails(emails: Email[], mailbox: Mailbox): Email[] {
  return emails.filter((email) => {
    if (mailbox === "inbox") return email.mailbox === "inbox";
    if (mailbox === "starred") return email.starred;
    return email.mailbox === mailbox;
  });
}

export function countMailbox(emails: Email[], mailbox: Mailbox): number {
  return visibleEmails(emails, mailbox).length;
}

function initialState(): MailState {
  return {
    emails: mockEmails(),
    selectedMailbox: "inbox",
    selectedIndex: 0,
    tabs: [],
    activeTab: null,
    workspaceView: { kind: "mailbox" },
    overlay: null,
    composeSeed: null,
    settingsPage: null,
    historyBack: [],
    historyForward: [],
  };
}

function currentEntry(state: MailState): NavEntry {
  return {
    view: state.workspaceView,
    mailbox: state.selectedMailbox,
    selectedIndex: state.selectedIndex,
  };
}

function sameEntry(a: NavEntry, b: NavEntry): boolean {
  return (
    a.mailbox === b.mailbox &&
    a.selectedIndex === b.selectedIndex &&
    (a.view.kind === "email" && b.view.kind === "email"
      ? a.view.id === b.view.id
      : a.view.kind === b.view.kind)
  );
}

/** Record where we are so the arrows can retrace it (mail.rs push_history). */
function pushHistory(state: MailState): MailState {
  const entry = currentEntry(state);
  const last = state.historyBack[state.historyBack.length - 1];
  return {
    ...state,
    historyBack: last && sameEntry(last, entry) ? state.historyBack : [...state.historyBack, entry],
    historyForward: [],
  };
}

function restoreEntry(state: MailState, entry: NavEntry): MailState {
  const activeTab = entry.view.kind === "email" && state.tabs.includes(entry.view.id)
    ? entry.view.id
    : null;
  return { ...state, selectedMailbox: entry.mailbox, selectedIndex: entry.selectedIndex, activeTab, workspaceView: entry.view };
}

function openEmailInState(
  state: MailState,
  id: EmailId,
  opts?: { markRead?: boolean; openInTab?: boolean },
): MailState {
  const found = state.emails.some((e) => e.id === id);
  if (!found) return state;
  const markRead = opts?.markRead ?? true;
  const openInTab = opts?.openInTab ?? true;
  const emails = markRead
    ? state.emails.map((e) => (e.id === id ? { ...e, unread: false } : e))
    : state.emails;
  const tabs = !openInTab || state.tabs.includes(id) ? state.tabs : [...state.tabs, id];
  return {
    ...state,
    emails,
    tabs,
    activeTab: openInTab ? id : null,
    workspaceView: { kind: "email", id },
  };
}

function closeTabInState(state: MailState, id: EmailId): MailState {
  const index = state.tabs.indexOf(id);
  if (index === -1) return state;
  const tabs = state.tabs.filter((t) => t !== id);
  const wasActive = state.activeTab === id;
  if (!wasActive) return { ...state, tabs };
  if (tabs.length === 0) {
    return { ...state, tabs, activeTab: null, workspaceView: { kind: "mailbox" } };
  }
  const next = index < tabs.length ? tabs[index] : tabs[index - 1];
  return { ...state, tabs, activeTab: next, workspaceView: { kind: "email", id: next } };
}

export function mailReducer(state: MailState, action: MailAction): MailState {
  switch (action.type) {
    case "select-mailbox":
      return {
        ...state,
        selectedMailbox: action.mailbox,
        selectedIndex: 0,
        workspaceView: { kind: "mailbox" },
        // The composer survives a mailbox switch, parked with its draft.
        overlay: state.overlay === "compose" ? "compose" : null,
        settingsPage: null,
      };
    case "move-selection": {
      const len = visibleEmails(state.emails, state.selectedMailbox).length;
      if (len === 0) return { ...state, selectedIndex: 0 };
      const next = (((state.selectedIndex + action.delta) % len) + len) % len;
      return { ...state, selectedIndex: next };
    }
    case "set-selected-index":
      return { ...state, selectedIndex: action.index };
    case "open-email":
      return pushHistory({
        ...openEmailInState(state, action.id, {
          markRead: action.markRead,
          openInTab: action.openInTab,
        }),
        settingsPage: null,
      });
    case "open-email-from-search":
      return {
        ...openEmailInState(state, action.id, {
          markRead: action.markRead,
          openInTab: action.openInTab,
        }),
        // The search scrim goes away; a compose pane underneath stays parked.
        overlay: state.overlay === "compose" ? state.overlay : null,
        settingsPage: null,
      };
    case "close-tab":
      return closeTabInState(state, action.id);
    case "close-active-tab":
      return state.activeTab == null ? state : closeTabInState(state, state.activeTab);
    case "cycle-tab": {
      if (state.tabs.length === 0) return state;
      const current =
        state.activeTab == null ? 0 : Math.max(0, state.tabs.indexOf(state.activeTab));
      const len = state.tabs.length;
      const next = (((current + action.direction) % len) + len) % len;
      const id = state.tabs[next];
      return {
        ...openEmailInState(state, id),
        activeTab: id,
        workspaceView: { kind: "email", id },
      };
    }
    case "toggle-star":
      return {
        ...state,
        emails: state.emails.map((e) =>
          e.id === action.id ? { ...e, starred: !e.starred } : e,
        ),
      };
    case "open-search":
      return { ...state, overlay: "search" };
    case "open-compose": {
      const incoming = action.seed;
      const blank =
        incoming.to.trim() === "" && incoming.subject.trim() === "" && incoming.body.trim() === "";
      // A blank open restores the parked draft; reply/forward seeds retire it.
      return { ...state, overlay: "compose", composeSeed: blank && state.composeSeed ? state.composeSeed : incoming };
    }
    case "close-overlay":
      return { ...state, overlay: null, composeSeed: null };
    case "close-compose":
      // Parked, not discarded: reopening restores the draft.
      return { ...state, overlay: null };
    case "park-compose":
      return { ...state, composeSeed: action.seed };
    case "open-settings":
      // Search is modal and would sit on top of settings, so it closes.
      // The composer is only hidden while settings is open, never destroyed:
      // its seed stays parked and the pane comes back with it.
      return {
        ...state,
        overlay: state.overlay === "compose" ? "compose" : null,
        settingsPage: "general",
      };
    case "close-settings":
      return { ...state, settingsPage: null };
    case "set-settings-page":
      return { ...state, settingsPage: action.page };
    case "go-back": {
      if (state.overlay === "search") return { ...state, overlay: null };
      if (state.overlay === "compose") return { ...state, overlay: null };
      if (state.settingsPage != null) return { ...state, settingsPage: null };
      const previous = state.historyBack[state.historyBack.length - 1];
      if (!previous) return { ...state, workspaceView: { kind: "mailbox" } };
      return restoreEntry(
        {
          ...state,
          historyBack: state.historyBack.slice(0, -1),
          historyForward: [...state.historyForward, currentEntry(state)],
        },
        previous,
      );
    }
    case "go-forward": {
      const next = state.historyForward[state.historyForward.length - 1];
      if (!next) return state;
      return restoreEntry(
        {
          ...state,
          historyBack: [...state.historyBack, currentEntry(state)],
          historyForward: state.historyForward.slice(0, -1),
        },
        next,
      );
    }
  }
}

/** Port of MailApp::reply_seed in views/mail_app.rs:196-226. */
export function replySeed(email: Email, replyAll: boolean, forward: boolean): DraftSeed {
  const subject = forward ? `Fwd: ${email.subject}` : `Re: ${email.subject}`;
  let to: string;
  if (forward) {
    to = "";
  } else if (replyAll) {
    to = email.recipients.filter((r) => r !== "me@example.com").join(", ");
  } else {
    to = email.address;
  }
  const body = forward
    ? `\n\n--- Forwarded message ---\nFrom: ${email.sender} <${email.address}>\n\n${email.body.join("\n\n")}`
    : `\n\nOn ${email.fullDate}:\n${email.body.join("\n\n")}`;
  return { to, subject, body };
}

export interface MailStoreApi {
  state: MailState;
  dispatch: Dispatch<MailAction>;
  visible: Email[];
  summaries: EmailSummary[];
  selectedEmail: Email | null;
  activeEmail: Email | null;
  counts: Record<Mailbox, number>;
}

export function useMailStore(): MailStoreApi {
  const [state, dispatch] = useReducer(mailReducer, undefined, initialState);
  return useMemo(() => {
    const visible = visibleEmails(state.emails, state.selectedMailbox);
    const summaries = visible.map(emailSummary);
    const selectedEmail = visible[state.selectedIndex] ?? null;
    const view = state.workspaceView;
    const activeEmail =
      view.kind === "email"
        ? (state.emails.find((e) => e.id === view.id) ?? null)
        : null;
    const counts: Record<Mailbox, number> = {
      inbox: countMailbox(state.emails, "inbox"),
      starred: countMailbox(state.emails, "starred"),
      sent: countMailbox(state.emails, "sent"),
      drafts: countMailbox(state.emails, "drafts"),
      archive: countMailbox(state.emails, "archive"),
      trash: countMailbox(state.emails, "trash"),
    };
    return { state, dispatch, visible, summaries, selectedEmail, activeEmail, counts };
  }, [state]);
}
