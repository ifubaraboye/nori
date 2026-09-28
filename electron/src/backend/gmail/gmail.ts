// Port of crates/nori-gmail/src/gmail.rs — typed Gmail REST client over fetch.
import { gmailFetch, GmailHttpError } from "./http.js";
import { urlencode } from "../auth/oauth.js";

export const GMAIL_BASE = "https://gmail.googleapis.com/gmail/v1";
export const PAGE_SIZE = 500;

export interface GmailLabel {
  id: string;
  name: string;
  kind: string;
  color?: string;
  messageListVisibility?: string;
  messagesTotal?: number;
  messagesUnread?: number;
}

export function isSystemLabel(label: GmailLabel): boolean {
  return label.kind === "system";
}

export function isVisibleLabel(label: GmailLabel): boolean {
  return label.messageListVisibility !== "hide";
}

export interface GmailHeader {
  name: string;
  value: string;
}

export interface MessageMetadata {
  id: string;
  threadId: string;
  labelIds: string[];
  snippet: string;
  internalDate?: string;
  payload?: { headers: GmailHeader[] };
}

export function headerOf(meta: MessageMetadata, name: string): string | undefined {
  const lower = name.toLowerCase();
  return meta.payload?.headers.find((h) => h.name.toLowerCase() === lower)?.value;
}

export function unixSecondsOf(meta: MessageMetadata): number | undefined {
  if (!meta.internalDate || meta.internalDate === "0") return undefined;
  const millis = Number(meta.internalDate);
  if (!Number.isFinite(millis)) return undefined;
  return Math.floor(millis / 1000);
}

export interface MessageList {
  messages: Array<{ id: string; threadId: string }>;
  nextPage?: string;
  estimate?: number;
}

export interface HistoryPage {
  entries: Array<{
    added: Array<{ id: string }>;
    deleted: Array<{ id: string }>;
    labels: Array<{ id: string }>;
  }>;
  historyId?: string;
  nextPage?: string;
}

export interface Profile {
  email: string;
  total: number;
  historyId?: string;
}

export interface SentMessage {
  id: string;
  threadId: string;
  labelIds: string[];
}

async function getJson<T>(url: string, token: string): Promise<T> {
  const text = await gmailFetch(url, token, { method: "GET" });
  try {
    return JSON.parse(text) as T;
  } catch (err) {
    throw new GmailHttpError({
      kind: "malformed",
      endpoint: url,
      reason: err instanceof Error ? err.message : String(err),
    });
  }
}

export async function profile(token: string): Promise<Profile> {
  const raw = await getJson<{
    emailAddress?: string;
    messagesTotal?: number;
    historyId?: string;
  }>(`${GMAIL_BASE}/users/me/profile`, token);
  return { email: raw.emailAddress ?? "", total: raw.messagesTotal ?? 0, historyId: raw.historyId };
}

export async function labels(token: string): Promise<GmailLabel[]> {
  const raw = await getJson<{ labels?: GmailLabel[] }>(`${GMAIL_BASE}/users/me/labels`, token);
  return raw.labels ?? [];
}

export async function label(token: string, id: string): Promise<GmailLabel> {
  return getJson<GmailLabel>(`${GMAIL_BASE}/users/me/labels/${urlencode(id)}`, token);
}

export async function listMessages(
  token: string,
  query: string,
  pageToken?: string,
): Promise<MessageList> {
  let url = `${GMAIL_BASE}/users/me/messages?maxResults=${PAGE_SIZE}&q=${urlencode(query)}`;
  if (pageToken) url += `&pageToken=${urlencode(pageToken)}`;
  const raw = await getJson<{
    messages?: Array<{ id: string; threadId?: string }>;
    nextPageToken?: string;
    resultSizeEstimate?: number;
  }>(url, token);
  return {
    messages: (raw.messages ?? []).map((m) => ({ id: m.id, threadId: m.threadId ?? "" })),
    nextPage: raw.nextPageToken,
    estimate: raw.resultSizeEstimate,
  };
}

export async function messageMetadata(token: string, id: string): Promise<MessageMetadata> {
  const raw = await getJson<{
    id?: string;
    threadId?: string;
    labelIds?: string[];
    snippet?: string;
    internalDate?: string;
    payload?: { headers?: GmailHeader[] };
  }>(
    `${GMAIL_BASE}/users/me/messages/${urlencode(id)}?format=metadata` +
      `&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date`,
    token,
  );
  return {
    id: raw.id ?? "",
    threadId: raw.threadId ?? "",
    labelIds: raw.labelIds ?? [],
    snippet: raw.snippet ?? "",
    internalDate: raw.internalDate,
    payload: raw.payload ? { headers: raw.payload.headers ?? [] } : undefined,
  };
}

export async function messageFull(token: string, id: string): Promise<unknown> {
  return getJson<unknown>(`${GMAIL_BASE}/users/me/messages/${urlencode(id)}?format=full`, token);
}

export async function history(token: string, startHistoryId: string): Promise<HistoryPage> {
  const url =
    `${GMAIL_BASE}/users/me/history?startHistoryId=${urlencode(startHistoryId)}` +
    `&historyTypes=messageAdded&historyTypes=messageDeleted` +
    `&historyTypes=labelAdded&historyTypes=labelRemoved`;
  const raw = await getJson<{
    history?: Array<{
      messagesAdded?: Array<{ message?: { id?: string } }>;
      messagesDeleted?: Array<{ message?: { id?: string } }>;
      labelsAdded?: Array<{ message?: { id?: string } }>;
      labelsRemoved?: Array<{ message?: { id?: string } }>;
    }>;
    historyId?: string;
    nextPageToken?: string;
  }>(url, token);
  return {
    entries: (raw.history ?? []).map((e) => ({
      added: (e.messagesAdded ?? []).map((m) => ({ id: m.message?.id ?? "" })).filter((m) => m.id),
      deleted: (e.messagesDeleted ?? []).map((m) => ({ id: m.message?.id ?? "" })).filter((m) => m.id),
      labels: [
        ...(e.labelsAdded ?? []).map((m) => ({ id: m.message?.id ?? "" })),
        ...(e.labelsRemoved ?? []).map((m) => ({ id: m.message?.id ?? "" })),
      ].filter((m) => m.id),
    })),
    historyId: raw.historyId,
    nextPage: raw.nextPageToken,
  };
}

export async function attachment(
  token: string,
  messageId: string,
  attachmentId: string,
): Promise<Uint8Array> {
  const raw = await getJson<{ data?: string }>(
    `${GMAIL_BASE}/users/me/messages/${urlencode(messageId)}/attachments/${urlencode(attachmentId)}`,
    token,
  );
  const data = (raw.data ?? "").replace(/-/g, "+").replace(/_/g, "/");
  try {
    return Uint8Array.from(Buffer.from(data, "base64"));
  } catch (err) {
    throw new GmailHttpError({
      kind: "malformed",
      endpoint: "attachments",
      reason: err instanceof Error ? err.message : String(err),
    });
  }
}

export async function modifyLabels(
  token: string,
  id: string,
  add: string[],
  remove: string[],
): Promise<void> {
  const body: Record<string, string[]> = {};
  if (add.length > 0) body.addLabelIds = add;
  if (remove.length > 0) body.removeLabelIds = remove;
  await gmailFetch(`${GMAIL_BASE}/users/me/messages/${urlencode(id)}/modify`, token, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

export async function sendMessage(token: string, raw: string): Promise<SentMessage> {
  const text = await gmailFetch(`${GMAIL_BASE}/users/me/messages/send`, token, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ raw }),
  });
  const parsed = JSON.parse(text) as { id?: string; threadId?: string; labelIds?: string[] };
  return { id: parsed.id ?? "", threadId: parsed.threadId ?? "", labelIds: parsed.labelIds ?? [] };
}
