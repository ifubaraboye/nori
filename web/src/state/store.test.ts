import { describe, expect, test } from "bun:test";
import {
  LABEL_COLOURS,
  mailReducer,
  visibleEmails,
  type MailState,
} from "./store";

function base(): MailState {
  let s: MailState;
  // walk the real initializer through the reducer via a null-ish start
  s = {
    emails: [
      { id: 1, sender: "A", address: "a@x.com", recipients: ["me@x.com"], subject: "One", preview: "p", body: ["b"], timestamp: "10:00", fullDate: "today", mailbox: "inbox", unread: true, starred: false, pinned: false },
      { id: 2, sender: "B", address: "b@x.com", recipients: ["me@x.com"], subject: "Two", preview: "p", body: ["b"], timestamp: "11:00", fullDate: "today", mailbox: "inbox", unread: false, starred: false, pinned: false },
      { id: 3, sender: "C", address: "c@x.com", recipients: ["me@x.com"], subject: "Trash", preview: "p", body: ["b"], timestamp: "12:00", fullDate: "today", mailbox: "trash", unread: false, starred: false, pinned: false },
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
  };
  return s;
}

describe("history arrows", () => {
  test("open then back returns to the mailbox, forward reopens", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: 1, markRead: true, openInTab: true });
    expect(s.workspaceView).toEqual({ kind: "email", id: 1 });
    expect(s.historyBack.length).toBe(1);
    // The recorded entry must be the mailbox we came FROM, not where we went.
    expect(s.historyBack[0].view).toEqual({ kind: "mailbox" });
    s = mailReducer(s, { type: "go-back" });
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
    expect(s.historyForward.length).toBe(1);
    s = mailReducer(s, { type: "go-forward" });
    expect(s.workspaceView).toEqual({ kind: "email", id: 1 });
  });

  test("mailbox switch is retraceable, one step per navigation", () => {
    let s = base();
    s = mailReducer(s, { type: "select-mailbox", mailbox: "trash" });
    s = mailReducer(s, { type: "open-email", id: 3, markRead: true, openInTab: true });
    expect(s.workspaceView).toEqual({ kind: "email", id: 3 });
    // Each navigation records its own entry, so back walks the same path:
    // the open mail, then the mailbox switch.
    s = mailReducer(s, { type: "go-back" });
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
    expect(s.selectedMailbox).toBe("trash");
    s = mailReducer(s, { type: "go-back" });
    expect(s.selectedMailbox).toBe("inbox");
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
  });

  test("back with no history still steps an open mail back to its list", () => {
    let s = base();
    s = { ...s, workspaceView: { kind: "email", id: 1 }, activeTab: 1 };
    s = mailReducer(s, { type: "go-back" });
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
  });
});

describe("pinned vs preview tabs", () => {
  test("unpinned mail opens as a preview, not a tab", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: 1, openInTab: true });
    expect(s.tabs).toEqual([]);
    expect(s.previewTab).toBe(1);
  });

  test("pinning promotes the preview into a real tab", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: 1, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: 1 });
    expect(s.tabs).toEqual([1]);
    expect(s.previewTab).toBeNull();
    expect(s.activeTab).toBe(1);
  });

  test("a new preview replaces the previous one, tabs untouched", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: 1, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: 1 });
    s = mailReducer(s, { type: "open-email", id: 2, openInTab: true });
    expect(s.tabs).toEqual([1]);
    expect(s.previewTab).toBe(2);
  });

  test("unpinning removes the tab and returns to the mailbox", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: 1, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: 1 });
    s = mailReducer(s, { type: "toggle-pin", id: 1 });
    expect(s.tabs).toEqual([]);
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
  });

  test("openInTab off leaves no active tab for unpinned mail", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: 1, openInTab: false });
    expect(s.activeTab).toBeNull();
    expect(s.tabs).toEqual([]);
    expect(s.workspaceView).toEqual({ kind: "email", id: 1 });
  });

  test("closing a preview returns to the mailbox, not a neighbour", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: 1, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: 1 });
    s = mailReducer(s, { type: "open-email", id: 2, openInTab: true });
    // Previews never accumulate, so there is no neighbour to step to even
    // though a pinned tab is still open behind it.
    s = mailReducer(s, { type: "close-tab", id: 2 });
    expect(s.previewTab).toBeNull();
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
    expect(s.tabs).toEqual([1]);
  });
});

describe("archive", () => {
  test("archiving an open mail returns to the mailbox and moves it", () => {
    let s = base();
    s = mailReducer(s, { type: "open-email", id: 1, openInTab: true });
    s = mailReducer(s, { type: "toggle-pin", id: 1 });
    s = mailReducer(s, { type: "request-archive", id: 1, confirm: false });
    expect(s.emails[0].mailbox).toBe("archive");
    expect(s.tabs).toEqual([1]);
    expect(s.workspaceView).toEqual({ kind: "mailbox" });
  });

  test("the archived mail leaves its old mailbox entirely", () => {
    let s = base();
    s = mailReducer(s, { type: "request-archive", id: 2, confirm: false });
    expect(s.emails[1].mailbox).toBe("archive");
    const shown = visibleEmails(s.emails, "inbox").map((e) => e.id);
    expect(shown).toEqual([1]);
    const archive = visibleEmails(s.emails, "archive").map((e) => e.id);
    expect(archive).toContain(2);
  });

  test("confirm off archives straight away; confirm on asks first", () => {
    let s = base();
    s = mailReducer(s, { type: "request-archive", id: 1, confirm: false });
    expect(s.emails[0].mailbox).toBe("archive");
    expect(s.confirmArchive).toBeNull();

    let t = base();
    t = mailReducer(t, { type: "request-archive", id: 1, confirm: true });
    expect(t.confirmArchive).toBe(1);
    expect(t.emails[0].mailbox).toBe("inbox");
    t = mailReducer(t, { type: "cancel-archive" });
    expect(t.confirmArchive).toBeNull();
    expect(t.emails[0].mailbox).toBe("inbox");

    t = mailReducer(t, { type: "request-archive", id: 1, confirm: true });
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
    s = mailReducer(s, { type: "toggle-label", id: 1, labelId: 1 });
    expect(s.assignments).toEqual([[1, [1]]]);
    s = mailReducer(s, { type: "toggle-label", id: 1, labelId: 1 });
    expect(s.assignments[0][1]).toEqual([]);
  });

  test("removing a label drops it from every mail and clears the filter", () => {
    let s = base();
    s = mailReducer(s, { type: "create-label", name: "Work" });
    s = mailReducer(s, { type: "toggle-label", id: 1, labelId: 1 });
    s = mailReducer(s, { type: "toggle-label", id: 2, labelId: 1 });
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
    s = mailReducer(s, { type: "toggle-label", id: 2, labelId: 1 });
    s = mailReducer(s, { type: "filter-by-label", labelId: 1 });
    const shown = visibleEmails(s.emails, "inbox").filter((e) => e.id === 2);
    expect(shown.length).toBe(1);
  });
});
