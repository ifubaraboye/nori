// Port of MailStore in crates/nori-ui/src/model/mail.rs:109-289.
// Pure helpers + a React reducer-based store. No backend calls.
import { useMemo, useReducer, type Dispatch } from "react";
import { mockEmails } from "../data/mock";
import { getNoriBridge } from "../bridge/noriBridge";
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
import type { AccountStatus } from "../types/mail";

export interface MailState {
  emails: Email[];
  selectedMailbox: Mailbox;
  selectedIndex: number;
  tabs: EmailId[];
  activeTab: EmailId | null;
  /** The provisional tab: the unpinned mail open right now, at most one. */
  previewTab: EmailId | null;
  workspaceView: WorkspaceView;
  overlay: Overlay | null;
  composeSeed: DraftSeed | null;
  /** Open settings page, or null. Holds the workspace while open. */
  settingsPage: SettingsPage | null;
  /** Visited views, for the sidebar back/forward arrows (mail.rs NavEntry). */
  historyBack: NavEntry[];
  historyForward: NavEntry[];
  /** Mail awaiting archive confirmation, or null (mail_app.rs confirm_archive). */
  confirmArchive: EmailId | null;
  /** Labels and their per-mail assignments (labels.rs LabelStore). */
  labels: Label[];
  assignments: Array<[EmailId, LabelId[]]>;
  nextLabelId: LabelId;
  /** Label menu open on this row, or null (mail_app.rs label_menu). */
  labelMenuFor: EmailId | null;
  /** Sidebar label filter, or null when the filter is off. */
  labelFilter: LabelId | null;
  /** Whether the account is connected, and what it is doing (account.rs). */
  account: AccountStatus;
  accountAddress: string | null;
  /** Why the last sign-in failed, in the user's words. */
  accountReason: string | null;
  /** True while the host is fetching; the list shows mail as it arrives. */
  syncing: boolean;
  /** Bodies not yet fetched, keyed by mail id (mail.rs body_loaded). */
  bodies: Record<EmailId, string[]>;
  /** Gmail label ids the mail carries, for write-back on label changes. */
  remoteLabels: Record<EmailId, string[]>;
  /** Gmail label id behind each local label, so a toggle can write back. */
  remoteLabelIds: Record<LabelId, string>;
}

export interface NavEntry {
  view: WorkspaceView;
  mailbox: Mailbox;
  selectedIndex: number;
}

export type LabelId = number;

export interface Label {
  id: LabelId;
  name: string;
  /** Packed 0xRRGGBB, as the Rust chip palette stores it (labels.rs). */
  colour: number;
}

export const LABEL_COLOURS = [
  0xe2658a, 0xe0a85b, 0x62c987, 0x5bc7b5, 0x62a8e2, 0xa08ce0, 0xd07ac8, 0x9aa5b5,
] as const;

export type MailAction =
  | { type: "select-mailbox"; mailbox: Mailbox }
  | { type: "move-selection"; delta: number }
  | { type: "set-selected-index"; index: number }
  | { type: "open-email"; id: EmailId; markRead?: boolean; openInTab?: boolean }
  | { type: "close-tab"; id: EmailId }
  | { type: "close-active-tab" }
  | { type: "cycle-tab"; direction: number }
  | { type: "toggle-star"; id: EmailId }
  | { type: "toggle-pin"; id: EmailId }
  | { type: "archive"; id: EmailId }
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
  | { type: "go-forward" }
  | { type: "request-archive"; id: EmailId; confirm: boolean }
  | { type: "confirm-archive" }
  | { type: "cancel-archive" }
  | { type: "open-label-menu"; id: EmailId }
  | { type: "close-label-menu" }
  | { type: "toggle-label"; id: EmailId; labelId: LabelId }
  | { type: "create-label"; name: string }
  | { type: "rename-label"; labelId: LabelId; name: string }
  | { type: "remove-label"; labelId: LabelId }
  | { type: "set-colour"; labelId: LabelId; colour: number }
  | { type: "filter-by-label"; labelId: LabelId | null }
  | {
      type: "set-account";
      status: AccountStatus;
      address?: string | null;
      reason?: string;
    }
  | { type: "set-syncing"; syncing: boolean }
  | {
      type: "load-snapshot";
      emails: Email[];
      labels: Label[];
      /** Mail id -> the *Gmail* label ids it carries, straight from the host. */
      assignments: Array<[EmailId, string[]]>;
      remoteLabels: Record<EmailId, string[]>;
      remoteLabelIds: Record<LabelId, string>;
    }
  | { type: "load-body"; id: EmailId; body: string[] }
  | { type: "apply-labels"; id: EmailId; unread?: boolean; starred?: boolean }
  | { type: "restore-remote-labels"; id: EmailId; remote: string[] };

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

/**
 * The sample mail is the standalone build's data source and nothing else's.
 *
 * Under Electron the host sends a snapshot before the first paint, and those
 * real mails replace the sample outright. Seeding the sample anyway is what
 * made a signed-in user see eighteen fabricated messages sitting in the list
 * next to their own.
 */
function initialState(): MailState {
  return {
    emails: getNoriBridge() != null ? [] : mockEmails(),
    selectedMailbox: "inbox",
    selectedIndex: 0,
    tabs: [],
    activeTab: null,
    previewTab: null,
    workspaceView: { kind: "mailbox" },
    overlay: null,
    composeSeed: null,
    settingsPage: null,
    historyBack: [],
    historyForward: [],
    confirmArchive: null,
    labels: [],
    assignments: [],
    nextLabelId: 1,
    labelMenuFor: null,
    labelFilter: null,
    account: "disconnected",
    accountAddress: null,
    accountReason: null,
    syncing: false,
    bodies: {},
    remoteLabels: {},
    remoteLabelIds: {},
  };
}

/** Labels carrying a given mail, in palette order (labels.rs labels_for). */
function labelsFor(state: MailState, id: EmailId): Label[] {
  const assigned = state.assignments.find(([mailId]) => mailId === id)?.[1] ?? [];
  return assigned
    .map((labelId) => state.labels.find((l) => l.id === labelId))
    .filter((l): l is Label => l != null);
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

/**
 * Record where we were so the arrows can retrace it (mail.rs push_history).
 *
 * Takes the pre-navigation state and returns the *post*-navigation state with
 * the entry appended: recording after the view has already moved is what made
 * the arrows light up and then do nothing, because every entry described
 * where the arrow was about to go.
 */
function pushHistory(before: MailState, after: MailState): MailState {
  const entry = currentEntry(before);
  const last = before.historyBack[before.historyBack.length - 1];
  return {
    ...after,
    historyBack: last && sameEntry(last, entry) ? before.historyBack : [...before.historyBack, entry],
    historyForward: [],
  };
}

function restoreEntry(state: MailState, entry: NavEntry): MailState {
  const activeTab = entry.view.kind === "email" && state.tabs.includes(entry.view.id)
    ? entry.view.id
    : null;
  return { ...state, selectedMailbox: entry.mailbox, selectedIndex: entry.selectedIndex, activeTab, workspaceView: entry.view };
}

/**
 * Port of MailStore::open_email (mail.rs:673-705).
 *
 * Only a pinned mail earns a tab. An unpinned one opens as the preview tab
 * when `openInTab` is on, replacing whatever preview came before — there is
 * ever at most one — and never touching the pinned tabs. With the setting
 * off, an unpinned mail opens with no active tab at all.
 */
function openEmailInState(
  state: MailState,
  id: EmailId,
  opts?: {
    markRead?: boolean;
    openInTab?: boolean;
    overlay?: Overlay | null;
    settingsPage?: SettingsPage | null;
  },
): MailState {
  const email = state.emails.find((e) => e.id === id);
  if (!email) return state;
  const markRead = opts?.markRead ?? true;
  const previewTabs = opts?.openInTab ?? true;
  const emails = markRead
    ? state.emails.map((e) => (e.id === id ? { ...e, unread: false } : e))
    : state.emails;
  const base: MailState = {
    ...state,
    emails,
    overlay: opts?.overlay ?? null,
    settingsPage: opts?.settingsPage ?? null,
  };

  if (email.pinned) {
    // Pinning the preview promotes it instead of duplicating it.
    const tabs = state.tabs.includes(id) ? state.tabs : [...state.tabs, id];
    return {
      ...base,
      tabs,
      previewTab: state.previewTab === id ? null : state.previewTab,
      activeTab: id,
      workspaceView: { kind: "email", id },
    };
  }
  if (previewTabs) {
    return {
      ...base,
      previewTab: id,
      activeTab: id,
      workspaceView: { kind: "email", id },
    };
  }
  return { ...base, activeTab: null, workspaceView: { kind: "email", id } };
}

/**
 * Port of MailStore::close_tab (mail.rs:815-849).
 *
 * A preview closes like a tab but is not one: dropping it returns to the
 * mailbox rather than to a neighbouring tab, because there is no neighbour —
 * previews never accumulate.
 */
function closeTabInState(state: MailState, id: EmailId): MailState {
  if (state.previewTab === id) {
    return {
      ...state,
      previewTab: null,
      activeTab: state.activeTab === id ? null : state.activeTab,
      workspaceView:
        state.workspaceView.kind === "email" && state.workspaceView.id === id
          ? { kind: "mailbox" }
          : state.workspaceView,
    };
  }
  const index = state.tabs.indexOf(id);
  if (index === -1) return state;
  const wasActive = state.activeTab === id;
  const tabs = state.tabs.filter((t) => t !== id);
  if (!wasActive) return { ...state, tabs };
  if (tabs.length === 0) {
    return { ...state, tabs, activeTab: null, workspaceView: { kind: "mailbox" } };
  }
  const next = index < tabs.length ? tabs[index] : tabs[index - 1];
  return { ...state, tabs, activeTab: next, workspaceView: { kind: "email", id: next } };
}

/**
 * Port of MailStore::archive (mail.rs:799-813) with mail_app.rs:2631.
 *
 * Archiving *is* the removal from the current mailbox, nothing more: the mail
 * exists and is in Archive afterwards, which is why there is no delete path
 * beside it. A pinned tab survives — it names a mail, not a folder — while an
 * open preview does not, and viewing the mail returns to the list.
 */
function archiveInState(state: MailState, id: EmailId): MailState {
  const email = state.emails.find((e) => e.id === id);
  if (!email) return state;
  const emails = state.emails.map((e) =>
    e.id === id ? { ...e, mailbox: "archive" as Mailbox } : e,
  );
  const wasOpen = state.workspaceView.kind === "email" && state.workspaceView.id === id;
  return {
    ...state,
    emails,
    confirmArchive: null,
    labelMenuFor: state.labelMenuFor === id ? null : state.labelMenuFor,
    previewTab: state.previewTab === id ? null : state.previewTab,
    activeTab: wasOpen ? null : state.activeTab,
    workspaceView: wasOpen ? { kind: "mailbox" } : state.workspaceView,
  };
}

export function mailReducer(state: MailState, action: MailAction): MailState {
  switch (action.type) {
    case "select-mailbox":
      if (state.selectedMailbox === action.mailbox && state.workspaceView.kind === "mailbox") {
        return state;
      }
      // Switching mailboxes is a step the arrows can retrace, so opening a
      // mail in Trash and going back lands in the inbox.
      return pushHistory(state, {
        ...state,
        selectedMailbox: action.mailbox,
        selectedIndex: 0,
        workspaceView: { kind: "mailbox" },
        activeTab: null,
        // A preview belongs to the mailbox it was opened from.
        previewTab: null,
        // The composer survives a mailbox switch, parked with its draft.
        overlay: state.overlay === "compose" ? "compose" : null,
        settingsPage: null,
      });
    case "move-selection": {
      const len = visibleEmails(state.emails, state.selectedMailbox).length;
      if (len === 0) return { ...state, selectedIndex: 0 };
      const next = (((state.selectedIndex + action.delta) % len) + len) % len;
      return { ...state, selectedIndex: next };
    }
    case "set-selected-index":
      return { ...state, selectedIndex: action.index };
    case "open-email":
      return pushHistory(
        state,
        openEmailInState(state, action.id, {
          markRead: action.markRead,
          openInTab: action.openInTab,
          settingsPage: null,
        }),
      );
    case "open-email-from-search":
      return pushHistory(
        state,
        openEmailInState(state, action.id, {
          markRead: action.markRead,
          openInTab: action.openInTab,
          // The search scrim goes away; a compose pane underneath stays parked.
          overlay: state.overlay === "compose" ? state.overlay : null,
          settingsPage: null,
        }),
      );
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
        ...openEmailInState(state, id, {
          // Cycling tabs is not a navigation away from the workspace.
          overlay: state.overlay,
          settingsPage: state.settingsPage,
        }),
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
    case "toggle-pin": {
      // Port of MailStore::toggle_pin (mail.rs:709-740): pinning adds the tab,
      // unpinning removes it and returns to the mailbox if it was the open view.
      const email = state.emails.find((e) => e.id === action.id);
      if (!email) return state;
      const pinned = !email.pinned;
      const emails = state.emails.map((e) => (e.id === action.id ? { ...e, pinned } : e));
      if (pinned) {
        return {
          ...state,
          emails,
          tabs: state.tabs.includes(action.id) ? state.tabs : [...state.tabs, action.id],
          previewTab: state.previewTab === action.id ? null : state.previewTab,
          activeTab:
            state.workspaceView.kind === "email" && state.workspaceView.id === action.id
              ? action.id
              : state.activeTab,
        };
      }
      const tabs = state.tabs.filter((t) => t !== action.id);
      const wasOpen =
        state.workspaceView.kind === "email" && state.workspaceView.id === action.id;
      return {
        ...state,
        emails,
        tabs,
        previewTab: state.previewTab === action.id ? null : state.previewTab,
        activeTab: wasOpen ? null : state.activeTab,
        workspaceView: wasOpen ? { kind: "mailbox" } : state.workspaceView,
      };
    }
    case "request-archive": {
      // mail_app.rs:2617 — the switch decides between asking and doing.
      if (!action.confirm) return archiveInState(state, action.id);
      return { ...state, confirmArchive: action.id, labelMenuFor: null };
    }
    case "confirm-archive":
      return state.confirmArchive == null
        ? state
        : archiveInState(state, state.confirmArchive);
    case "cancel-archive":
      return state.confirmArchive == null ? state : { ...state, confirmArchive: null };
    case "open-label-menu":
      return { ...state, labelMenuFor: action.id };
    case "close-label-menu":
      return state.labelMenuFor == null ? state : { ...state, labelMenuFor: null };
    case "toggle-label": {
      const holds = state.assignments.find(([id]) => id === action.id)?.[1] ?? [];
      const next = holds.includes(action.labelId)
        ? holds.filter((l) => l !== action.labelId)
        : [...holds, action.labelId];
      return {
        ...state,
        assignments: [
          ...state.assignments.filter(([id]) => id !== action.id),
          [action.id, next],
        ],
      };
    }
    case "create-label": {
      // labels.rs:115 — trimmed, non-empty, unique case-insensitively.
      const name = action.name.trim();
      if (name === "") return state;
      if (state.labels.some((l) => l.name.toLowerCase() === name.toLowerCase())) return state;
      const id = state.nextLabelId;
      return {
        ...state,
        nextLabelId: id + 1,
        labels: [
          ...state.labels,
          { id, name, colour: LABEL_COLOURS[(id - 1) % LABEL_COLOURS.length] },
        ],
      };
    }
    case "rename-label": {
      const name = action.name.trim();
      if (name === "") return state;
      return {
        ...state,
        labels: state.labels.map((l) => (l.id === action.labelId ? { ...l, name } : l)),
      };
    }
    case "remove-label": {
      // Removing a label drops it from every mail that carried it, and
      // clears the sidebar filter if it was the one being shown.
      return {
        ...state,
        labels: state.labels.filter((l) => l.id !== action.labelId),
        assignments: state.assignments
          .map(([id, held]) => [id, held.filter((l) => l !== action.labelId)] as [EmailId, LabelId[]])
          .filter(([, held]) => held.length > 0),
        labelFilter: state.labelFilter === action.labelId ? null : state.labelFilter,
      };
    }
    case "set-colour":
      return {
        ...state,
        labels: state.labels.map((l) =>
          l.id === action.labelId ? { ...l, colour: action.colour } : l,
        ),
      };
    case "filter-by-label":
      return {
        ...state,
        labelFilter: state.labelFilter === action.labelId ? null : action.labelId,
      };
    case "set-account":
      return {
        ...state,
        account: action.status,
        accountAddress:
          action.address !== undefined ? action.address : state.accountAddress,
        accountReason: action.reason ?? null,
      };
    case "set-syncing":
      return { ...state, syncing: action.syncing };
    case "load-snapshot": {
      // The host knows read/star state but nothing about pinning, which is
      // local, so a pinned flag is carried across the refresh rather than
      // overwritten. Pinned mail the host no longer lists is kept too, so a
      // tab that is open does not vanish from under the reader.
      //
      // Nothing else survives: the first snapshot from a connected account
      // replaces the sample mail outright. Keeping it would mean a signed-in
      // user stares at eighteen fabricated messages on top of their real ones,
      // which is the one thing that makes a connected client untrustworthy.
      const previous = new Map(state.emails.map((e) => [e.id, e]));
      const merged = action.emails.map((email) => {
        const held = previous.get(email.id);
        return held ? { ...email, pinned: held.pinned } : email;
      });
      const incoming = new Set(action.emails.map((e) => e.id));
      const orphans = state.emails.filter((e) => e.pinned && !incoming.has(e.id));
      // The cursor moves on every snapshot, so a tab for a mail the server no
      // longer lists would otherwise keep addressing a mail that is gone.
      const liveIds = new Set(merged.map((e) => e.id));
      const tabs = state.tabs.filter((id) => liveIds.has(id));
      const activeTab =
        state.activeTab != null && liveIds.has(state.activeTab) ? state.activeTab : null;
      const previewTab =
        state.previewTab != null && liveIds.has(state.previewTab) ? state.previewTab : null;
      // Gmail label ids and Nori's local ids are different namespaces, and
      // this is the one place holding both sides of the mapping, so the
      // translation happens here rather than in the renderer.
      const toLocal = new Map<string, LabelId>();
      for (const [local, remote] of Object.entries(action.remoteLabelIds)) {
        toLocal.set(remote, Number(local) as LabelId);
      }
      return {
        ...state,
        emails: [...merged, ...orphans],
        tabs,
        activeTab,
        previewTab,
        selectedIndex: 0,
        workspaceView:
          state.workspaceView.kind === "email" && !liveIds.has(state.workspaceView.id)
            ? { kind: "mailbox" }
            : state.workspaceView,
        labels: action.labels,
        assignments: action.assignments
          .map(
            ([id, remotes]): [EmailId, LabelId[]] => [
              id,
              remotes
                .map((remote) => toLocal.get(remote))
                .filter((local): local is LabelId => local !== undefined),
            ],
          )
          .filter(([, held]) => held.length > 0),
        remoteLabels: action.remoteLabels,
        remoteLabelIds: action.remoteLabelIds,
      };
    }
    case "load-body":
      return { ...state, bodies: { ...state.bodies, [action.id]: action.body } };
    case "apply-labels":
      return {
        ...state,
        emails: state.emails.map((e) =>
          e.id === action.id
            ? {
                ...e,
                unread: action.unread ?? e.unread,
                starred: action.starred ?? e.starred,
              }
            : e,
        ),
      };
    case "restore-remote-labels":
      return {
        ...state,
        remoteLabels: { ...state.remoteLabels, [action.id]: action.remote },
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
      if (!previous) {
        // Nothing recorded: an open mail still steps back to its list.
        return state.workspaceView.kind === "email"
          ? { ...state, activeTab: null, previewTab: null, workspaceView: { kind: "mailbox" } }
          : state;
      }
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
    default:
      return state;
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
  // Replying to a mail whose body has not loaded yet quotes what is there,
  // which is nothing, rather than throwing and leaving the compose pane dead.
  const quoted = (email.body ?? []).join("\n\n");
  const body = forward
    ? `\n\n--- Forwarded message ---\nFrom: ${email.sender} <${email.address}>\n\n${quoted}`
    : `\n\nOn ${email.fullDate}:\n${quoted}`;
  return { to, subject, body };
}

/** A mail with its fetched body attached, for the reading view. */
export function withBody(state: MailState, email: Email): Email {
  const body = state.bodies[email.id];
  // Defaulting to [] keeps the reading view renderable for a mail whose body
  // has not been fetched yet.
  return { ...email, body: body ?? email.body ?? [] };
}

export interface MailStoreApi {
  state: MailState;
  dispatch: Dispatch<MailAction>;
  visible: Email[];
  summaries: EmailSummary[];
  selectedEmail: Email | null;
  counts: Record<Mailbox, number>;
  /** Pinned tabs, then the preview tab last (email_tabs.rs order). */
  tabs: EmailId[];
  previewTab: EmailId | null;
  labelsByEmail: Map<EmailId, Label[]>;
  /** The open mail with its fetched body attached, when one has arrived. */
  activeEmail: Email | null;
}

/** Chip colours: solid text over a translucent wash of the same hue. */
export function labelChip(colour: number): { text: string; border: string; fill: string } {
  const r = (colour >> 16) & 0xff;
  const g = (colour >> 8) & 0xff;
  const b = colour & 0xff;
  return {
    text: `rgb(${r}, ${g}, ${b})`,
    border: `rgba(${r}, ${g}, ${b}, 0.45)`,
    fill: `rgba(${r}, ${g}, ${b}, 0.16)`,
  };
}

export function useMailStore(): MailStoreApi {
  const [state, dispatch] = useReducer(mailReducer, undefined, initialState);
  return useMemo(() => {
    const visible = visibleEmails(state.emails, state.selectedMailbox);
    // A label filter narrows the list but not the counts: the badges describe
    // the mailbox, not the current slice of it.
    const shown =
      state.labelFilter == null
        ? visible
        : visible.filter((e) => labelsFor(state, e.id).some((l) => l.id === state.labelFilter));
    const summaries = shown.map(emailSummary);
    const selectedEmail = shown[state.selectedIndex] ?? null;
    const view = state.workspaceView;
    const opened =
      view.kind === "email" ? (state.emails.find((e) => e.id === view.id) ?? null) : null;
    const activeEmail = opened ? withBody(state, opened) : null;
    // Preview first, then pinned tabs: the strip reads "what is open, then
    // what is kept", with the provisional tab last and in italics.
    const previewTab =
      state.previewTab != null && state.previewTab !== state.activeTab ? state.previewTab : null;
    const tabs = [...state.tabs, ...(previewTab != null ? [previewTab] : [])];
    const counts: Record<Mailbox, number> = {
      inbox: countMailbox(state.emails, "inbox"),
      starred: countMailbox(state.emails, "starred"),
      sent: countMailbox(state.emails, "sent"),
      drafts: countMailbox(state.emails, "drafts"),
      archive: countMailbox(state.emails, "archive"),
      trash: countMailbox(state.emails, "trash"),
    };
    return {
      state,
      dispatch,
      visible,
      summaries,
      selectedEmail,
      activeEmail,
      counts,
      tabs,
      previewTab,
      /** Labels per mail id, for the row chips. */
      labelsByEmail: new Map(state.emails.map((e) => [e.id, labelsFor(state, e.id)])),
    };
  }, [state]);
}
