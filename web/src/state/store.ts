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

export interface MailState {
  emails: Email[];
  selectedMailbox: Mailbox;
  selectedIndex: number;
  tabs: EmailId[];
  activeTab: EmailId | null;
  workspaceView: WorkspaceView;
  overlay: Overlay | null;
  composeSeed: DraftSeed | null;
}

export type MailAction =
  | { type: "select-mailbox"; mailbox: Mailbox }
  | { type: "move-selection"; delta: number }
  | { type: "set-selected-index"; index: number }
  | { type: "open-email"; id: EmailId }
  | { type: "close-tab"; id: EmailId }
  | { type: "close-active-tab" }
  | { type: "cycle-tab"; direction: number }
  | { type: "toggle-star"; id: EmailId }
  | { type: "open-search" }
  | { type: "open-compose"; seed: DraftSeed }
  | { type: "close-overlay" }
  | { type: "open-email-from-search"; id: EmailId }
  | { type: "go-back" };

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
  };
}

function openEmailInState(state: MailState, id: EmailId): MailState {
  const found = state.emails.some((e) => e.id === id);
  if (!found) return state;
  const emails = state.emails.map((e) =>
    e.id === id ? { ...e, unread: false } : e,
  );
  const tabs = state.tabs.includes(id) ? state.tabs : [...state.tabs, id];
  return {
    ...state,
    emails,
    tabs,
    activeTab: id,
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
        overlay: null,
        composeSeed: null,
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
      return openEmailInState(state, action.id);
    case "open-email-from-search":
      return { ...openEmailInState(state, action.id), overlay: null, composeSeed: null };
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
    case "open-compose":
      return { ...state, overlay: "compose", composeSeed: action.seed };
    case "close-overlay":
      return { ...state, overlay: null, composeSeed: null };
    case "go-back": {
      if (state.overlay != null) return { ...state, overlay: null, composeSeed: null };
      return { ...state, workspaceView: { kind: "mailbox" } };
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
