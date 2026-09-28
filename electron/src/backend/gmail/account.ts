// Port of nori-ui model/account.rs + labels.rs essentials for the main process.
import type { RemoteMail } from "./sync.js";

export type Mailbox = "inbox" | "starred" | "sent" | "drafts" | "archive" | "trash";

export function gmailQuery(mailbox: Mailbox): string {
  switch (mailbox) {
    case "inbox":
      return "in:inbox";
    case "starred":
      return "is:starred";
    case "sent":
      return "in:sent";
    case "drafts":
      return "in:drafts";
    case "archive":
      return "in:anywhere -in:inbox -in:sent -in:drafts -in:trash -in:spam -is:starred";
    case "trash":
      return "{in:trash in:spam}";
  }
}

const MAILBOX_PRIORITY: Array<[string, Mailbox]> = [
  ["TRASH", "trash"],
  ["SPAM", "trash"],
  ["DRAFT", "drafts"],
  ["SENT", "sent"],
  ["SENT_MAIL", "sent"],
  ["INBOX", "inbox"],
];

export function mailboxOf(labelIds: string[]): Mailbox {
  for (const [label, mailbox] of MAILBOX_PRIORITY) {
    if (labelIds.includes(label)) return mailbox;
  }
  return "archive";
}

export interface UiEmail {
  id: string;
  sender: string;
  address: string;
  recipients: string[];
  subject: string;
  preview: string;
  timestamp: string;
  fullDate: string;
  mailbox: Mailbox;
  unread: boolean;
  starred: boolean;
  threadId: string;
}

export function toUiEmail(remote: RemoteMail, account: string): UiEmail {
  void account;
  return {
    id: remote.id,
    sender: remote.sender,
    address: remote.address,
    recipients: remote.recipients,
    subject: remote.subject,
    preview: remote.preview,
    timestamp: remote.timestamp,
    fullDate: remote.fullDate,
    mailbox: mailboxOf(remote.labelIds),
    unread: remote.labelIds.includes("UNREAD"),
    starred: remote.labelIds.includes("STARRED"),
    threadId: remote.threadId,
  };
}

export const LABEL_COLOURS = [
  0xe2658a, 0xe0a85b, 0x62c987, 0x5bc7b5, 0x62a8e2, 0xa08ce0, 0xd07ac8, 0x9aa5b5,
];
