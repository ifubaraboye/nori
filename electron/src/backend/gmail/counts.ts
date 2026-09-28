// Port of crates/nori-gmail/src/counts.rs.
export interface FolderCounts {
  inbox: number;
  starred: number;
  sent: number;
  drafts: number;
  archive: number;
  trash: number;
  inboxUnread: number;
}

// labels.list carries no totals; only labels.get per system label does.
// lookup(id) -> [total, unread] | undefined. mailboxTotal = profile.messagesTotal.
export function folderCountsFromLookup(
  lookup: (id: string) => [number, number] | undefined,
  mailboxTotal: number,
): FolderCounts {
  const total = (id: string): number => lookup(id)?.[0] ?? 0;
  const inbox = total("INBOX");
  const sent = total("SENT");
  const drafts = total("DRAFT");
  const trash = total("TRASH") + total("SPAM");
  const starred = total("STARRED");
  const inboxUnread = lookup("INBOX")?.[1] ?? 0;
  const archive = Math.max(0, mailboxTotal - inbox - sent - drafts - trash);
  return { inbox, starred, sent, drafts, archive, trash, inboxUnread };
}
