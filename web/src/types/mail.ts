// Port of crates/nori-ui/src/model/mail.rs (types only, no GPUI).

export type EmailId = number;

export type Mailbox = "inbox" | "starred" | "sent" | "drafts" | "archive" | "trash";

export const MAILBOX_NAV_ITEMS: Mailbox[] = [
  "inbox",
  "starred",
  "sent",
  "drafts",
  "archive",
  "trash",
];

export function mailboxLabel(mailbox: Mailbox): string {
  switch (mailbox) {
    case "inbox":
      return "Inbox";
    case "starred":
      return "Starred";
    case "sent":
      return "Sent";
    case "drafts":
      return "Drafts";
    case "archive":
      return "Archive";
    case "trash":
      return "Trash";
  }
}

export interface Email {
  id: EmailId;
  sender: string;
  address: string;
  recipients: string[];
  subject: string;
  preview: string;
  body: string[];
  timestamp: string;
  fullDate: string;
  mailbox: Mailbox;
  unread: boolean;
  starred: boolean;
}

export interface EmailSummary {
  id: EmailId;
  sender: string;
  subject: string;
  preview: string;
  timestamp: string;
  unread: boolean;
  starred: boolean;
}

export function emailSummary(email: Email): EmailSummary {
  return {
    id: email.id,
    sender: email.sender,
    subject: email.subject,
    preview: email.preview,
    timestamp: email.timestamp,
    unread: email.unread,
    starred: email.starred,
  };
}

/** Port of Email::matches in model/mail.rs. */
export function emailMatches(email: Email, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (q === "") return true;
  return (
    email.sender.toLowerCase().includes(q) ||
    email.subject.toLowerCase().includes(q) ||
    email.preview.toLowerCase().includes(q) ||
    email.body.some((p) => p.toLowerCase().includes(q))
  );
}

export interface DraftSeed {
  to: string;
  subject: string;
  body: string;
}

export type WorkspaceView = { kind: "mailbox" } | { kind: "email"; id: EmailId };

export type Overlay = "search" | "compose";
