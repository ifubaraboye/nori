import { describe, expect, test } from "bun:test";
import {
  LABEL_COLOURS,
  mailReducer,
  visibleEmails,
  withBody,
  type MailState,
} from "./store";
import type { Email, EmailId } from "../types/mail";

/** Ids are strings, as Gmail's are. */
const ONE = "1" as EmailId;
const TWO = "2" as EmailId;
const TRASH = "3" as EmailId;

function makeEmail(
  id: string,
  sender: string,
  subject: string,
  mailbox: "inbox" | "trash",
  unread = false,
): Email {
  return {
    id,
    sender,
    address: `${sender.toLowerCase()}@x.com`,
    recipients: ["me@x.com"],
    subject,
    preview: "p",
    body: ["b"],
    timestamp: "10:00",
    fullDate: "today",
    mailbox,
    unread,
    starred: false,
    pinned: false,
  };
}

function base(): MailState {
  return {
    emails: [
      makeEmail(ONE, "A", "One", "inbox", true),
      makeEmail(TWO, "B", "Two", "inbox"),
      makeEmail(TRASH, "C", "Trash", "trash"),
    ],
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

describe("history arrows", () => {
  test("open then back returns to the mailbox, forward reopens", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, markRead: true, openInTab: true });
    expect(s.workspaceView).toEqual({ kind: "email", id: ONE });
    expect(s.historyBack.length).toBe(1);
    // The recorded entry must be the mailbox we came FROM, not where we went.
    expect(s.historyBack[0].view).toEqual({ kind: "mailbox" });
    s = mailReducer(s, { type: "go-back" });
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
    expect(s.historyForward.length).toBe(1);
    s = mailReducer(s, { type: "go-forward" });
    expect(s.workspaceView).toEqual({ kind: "email", id: ONE });
  });

  test("mailbox switch is retraceable, one step per navigation", () => {
    let s = base();
    s = mailReducer(s, { type: "select-mailbox", mailbox: "trash" });
    s = mailReducer(s, { type: "open-email", id: TRASH, markRead: true, openInTab: true });
    expect(s.workspaceView).toEqual({ kind: "email", id: TRASH });
    // Each navigation records its own entry, so back walks the same path:
    // the open mail, then the mailbox switch.
    s = mailReducer(s, { type: "go-back" });
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
    expect(s.selectedMailbox).toBe("trash");
    s = mailReducer(s, { type: "go-back" });
    expect(s.selectedMailbox).toBe("inbox");
  });

  test("back with no history still steps an open mail back to its list", () => {
    let s = base();
    s = { ...s, workspaceView: { kind: "email", id: ONE }, activeTab: ONE };
    s = mailReducer(s, { type: "go-back" });
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
  });
});

describe("pinned vs preview tabs", () => {
  test("unpinned mail opens as a preview, not a tab", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, openInTab: true });
    expect(s.tabs).toEqual([]);
    expect(s.previewTab).toBe(ONE);
  });

  test("pinning promotes the preview into a real tab", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: ONE });
    expect(s.tabs).toEqual([ONE]);
    expect(s.previewTab).toBeNull();
    expect(s.activeTab).toBe(ONE);
  });

  test("a new preview replaces the previous one, tabs untouched", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: ONE });
    s = mailReducer(s, { type: "open-email", id: TWO, openInTab: true });
    expect(s.tabs).toEqual([ONE]);
    expect(s.previewTab).toBe(TWO);
  });

  test("unpinning removes the tab and returns to the mailbox", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: ONE });
    s = mailReducer(s, { type: "toggle-pin", id: ONE });
    expect(s.tabs).toEqual([]);
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
  });

  test("openInTab off leaves no active tab for unpinned mail", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, openInTab: false });
    expect(s.activeTab).toBeNull();
    expect(s.tabs).toEqual([]);
    expect(s.workspaceView).toEqual({ kind: "email", id: ONE });
  });

  test("closing a preview returns to the mailbox, not a neighbour", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: ONE });
    s = mailReducer(s, { type: "open-email", id: TWO, openInTab: true });
    // Previews never accumulate, so there is no neighbour to step to even
    // though a pinned tab is still open behind it.
    s = mailReducer(s, { type: "close-tab", id: TWO });
    expect(s.previewTab).toBeNull();
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
    expect(s.tabs).toEqual([ONE]);
  });
});

describe("archive", () => {
  test("archiving an open mail returns to the mailbox and moves it", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: ONE });
    s = mailReducer(s, { type: "request-archive", id: ONE, confirm: false });
    expect(s.emails[0].mailbox).toBe("archive");
    expect(s.tabs).toEqual([ONE]);
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
  });

  test("the archived mail leaves its old mailbox entirely", () => {
    let s = base();
    s = mailReducer(s, { type: "request-archive", id: TWO, confirm: false });
    expect(s.emails[1].mailbox).toBe("archive");
    expect(visibleEmails(s.emails, "inbox").map((e) => e.id)).toEqual([ONE]);
    expect(visibleEmails(s.emails, "archive").map((e) => e.id)).toContain(TWO);
  });

  test("confirm off archives straight away; confirm on asks first", () => {
    let s = base();
    s = mailReducer(s, { type: "request-archive", id: ONE, confirm: false });
    expect(s.emails[0].mailbox).toBe("archive");
    expect(s.confirmArchive).toBeNull();

    let t = base();
    t = mailReducer(t, { type: "request-archive", id: ONE, confirm: true });
    expect(t.confirmArchive).toBe(ONE);
    expect(t.emails[0].mailbox).toBe("inbox");
    t = mailReducer(t, { type: "cancel-archive" });
    expect(t.confirmArchive).toBeNull();
    expect(t.emails[0].mailbox).toBe("inbox");

    t = mailReducer(t, { type: "request-archive", id: ONE, confirm: true });
    t = mailReducer(t, { type: "confirm-archive" });
    expect(t.emails[0].mailbox).toBe("archive");
    expect(t.confirmArchive).toBeNull();
  });
});

describe("labels", () => {
  test("create trims, refuses blanks and duplicates, colours by rotation", () => {
    let s = base();
    s = mailReducer(s, { type: "create-label", name: "  Work  " });
    expect(s.labels).toEqual([{ id: 1, name: "Work", colour: LABEL_COLOURS[0] }]);
    s = mailReducer(s, { type: "create-label", name: "work" });
    expect(s.labels.length).toBe(1);
    s = mailReducer(s, { type: "create-label", name: "   " });
    expect(s.labels.length).toBe(1);
    s = mailReducer(s, { type: "create-label", name: "Home" });
    expect(s.labels[1]).toEqual({ id: 2, name: "Home", colour: LABEL_COLOURS[1] });
  });

  test("assign and unassign a label on a mail", () => {
    let s = base();
    s = mailReducer(s, { type: "create-label", name: "Work" });
    s = mailReducer(s, { type: "toggle-label", id: ONE, labelId: 1 });
    expect(s.assignments).toEqual([[ONE, [1]]]);
    s = mailReducer(s, { type: "toggle-label", id: ONE, labelId: 1 });
    expect(s.assignments[0][1]).toEqual([]);
  });

  test("removing a label drops it from every mail and clears the filter", () => {
    let s = base();
    s = mailReducer(s, { type: "create-label", name: "Work" });
    s = mailReducer(s, { type: "toggle-label", id: ONE, labelId: 1 });
    s = mailReducer(s, { type: "toggle-label", id: TWO, labelId: 1 });
    s = mailReducer(s, { type: "filter-by-label", labelId: 1 });
    expect(s.labelFilter).toBe(1);
    s = mailReducer(s, { type: "remove-label", labelId: 1 });
    expect(s.labels).toEqual([]);
    expect(s.assignments).toEqual([]);
    expect(s.labelFilter).toBeNull();
  });

  test("a label filter narrows the visible list", () => {
    let s = base();
    s = mailReducer(s, { type: "create-label", name: "Work" });
    s = mailReducer(s, { type: "toggle-label", id: TWO, labelId: 1 });
    s = mailReducer(s, { type: "filter-by-label", labelId: 1 });
    expect(s.labelFilter).toBe(1);
    expect(visibleEmails(s.emails, "inbox").length).toBe(2);
  });
});

describe("host snapshot", () => {
  test("a snapshot replaces the list and keeps locally pinned mail", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: ONE });
    s = mailReducer(s, {
      type: "load-snapshot",
      emails: [makeEmail(ONE, "A", "One", "inbox"), makeEmail("99", "Z", "Fresh", "inbox")],
      labels: [],
      assignments: [],
      remoteLabels: {},
      remoteLabelIds: {},
    });
    // The host knows read state but nothing about pinning, which is local, so
    // a pin must survive the refresh rather than being overwritten.
    expect(s.emails.find((e) => e.id === ONE)?.pinned).toBe(true);
    expect(s.emails.find((e) => e.id === "99")?.subject).toBe("Fresh");
  });

  test("mail the host no longer lists survives while it is pinned", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: TWO, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: TWO });
    s = mailReducer(s, {
      type: "load-snapshot",
      emails: [makeEmail(ONE, "A", "One", "inbox")],
      labels: [],
      assignments: [],
      remoteLabels: {},
      remoteLabelIds: {},
    });
    expect(s.emails.find((e) => e.id === TWO)).toBeDefined();
  });

  test("a body is stored against its mail and read back on the open mail", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, openInTab: true });
    s = mailReducer(s, { type: "load-body", id: ONE, body: ["Hello", "World"] });
    const opened = s.emails.find((e) => e.id === ONE);
    expect(opened).toBeDefined();
    expect(withBody(s, opened as Email).body).toEqual(["Hello", "World"]);
  });

  test("gmail label ids and local label ids are kept apart", () => {
    let s = base();
    s = mailReducer(s, {
      type: "load-snapshot",
      emails: base().emails,
      labels: [
        { id: 1, name: "Work", colour: 0xe2658a },
        { id: 2, name: "Home", colour: 0xe0a85b },
      ],
      // "Label_9" is the Gmail id of the label Nori calls Home (local 2).
      assignments: [[ONE, ["Label_9"]]],
      remoteLabels: { [ONE]: ["Label_9"] },
      remoteLabelIds: { 1: "Label_7", 2: "Label_9" },
    });
    expect(s.assignments).toEqual([[ONE, [2]]]);
    expect(s.remoteLabelIds[2]).toBe("Label_9");
  });

  test("the sample mail does not survive a real snapshot", () => {
    // The bug this covers: a signed-in user saw the eighteen fabricated
    // messages sitting in the list next to their own mail, because the
    // snapshot only merged rather than replaced.
    let s = base();
    s = mailReducer(s, {
      type: "load-snapshot",
      emails: [makeEmail("g1", "Real", "Real mail", "inbox", true)],
      labels: [],
      assignments: [],
      remoteLabels: {},
      remoteLabelIds: {},
    });
    expect(s.emails.map((e) => e.subject)).toEqual(["Real mail"]);
    expect(s.emails.some((e) => e.subject === "One")).toBe(false);
  });

  test("a snapshot drops tabs and the open view for mail it no longer lists", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: ONE, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: ONE });
    expect(s.tabs).toEqual([ONE]);
    s = mailReducer(s, {
      type: "load-snapshot",
      emails: [makeEmail("g1", "Real", "Real mail", "inbox")],
      labels: [],
      assignments: [],
      remoteLabels: {},
      remoteLabelIds: {},
    });
    // ONE was pinned, so it survives as a tab; the preview cursor for a mail
    // the server dropped does not.
    expect(s.previewTab).toBeNull();
    expect(s.workspaceView.kind).toBe("mailbox");
  });

  test("a failed sign-in records why, rather than sitting on 'fetching'", () => {
    // The bug this covers: a sign-in that could never complete left the page
    // claiming "Fetching your mail… Signed in as ." forever.
    let s = base();
    s = mailReducer(s, { type: "set-account", status: "connecting" });
    expect(s.account).toBe("connecting");
    s = mailReducer(s, {
      type: "set-account",
      status: "failed",
      address: null,
      reason: "no browser was opened",
    });
    expect(s.account).toBe("failed");
    expect(s.accountAddress).toBeNull();
    expect(s.accountReason).toBe("no browser was opened");
  });

  test("a sign-in that succeeds clears any earlier failure", () => {
    let s = base();
    s = mailReducer(s, { type: "set-account", status: "failed", reason: "boom" });
    s = mailReducer(s, { type: "set-account", status: "connected", address: "me@x.com" });
    expect(s.account).toBe("connected");
    expect(s.accountAddress).toBe("me@x.com");
    expect(s.accountReason).toBeNull();
  });

  test("settled labels from the server win over the optimistic guess", () => {
    let s = base();
    s = mailReducer(s, { type: "toggle-star", id: ONE });
    expect(s.emails[0].starred).toBe(true);
    // The host says the label did not take; the store must believe it.
    s = mailReducer(s, { type: "apply-labels", id: ONE, starred: false });
    expect(s.emails[0].starred).toBe(false);
  });
});
