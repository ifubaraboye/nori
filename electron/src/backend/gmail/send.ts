// Port of crates/nori-gmail/src/send.rs — RFC 2822 MIME builder for Gmail send.
import { randomBytes } from "node:crypto";

export const MAX_SEND_BYTES = 25 * 1024 * 1024;

export interface SendAttachment {
  filename: string;
  mimeType: string;
  bytes: Uint8Array;
}

export function newSendAttachment(filename: string, mimeType: string, bytes: Uint8Array): SendAttachment {
  return { filename, mimeType, bytes };
}

export function parseRecipients(to: string): string[] {
  const recipients: string[] = [];
  for (const part of to.split(/[,;]/)) {
    const address = part.trim();
    if (!address) continue;
    if (!looksLikeAddress(address)) {
      throw new Error(`${address} doesn't look like an email address`);
    }
    recipients.push(address);
  }
  if (recipients.length === 0) throw new Error("add a recipient before sending");
  return recipients;
}

function looksLikeAddress(address: string): boolean {
  if (/\s/.test(address) || address.includes("<") || address.includes(">")) return false;
  const at = address.indexOf("@");
  if (at === -1) return false;
  const local = address.slice(0, at);
  const domain = address.slice(at + 1);
  if (!local || !domain) return false;
  if (!domain.includes(".") || domain.startsWith(".")) return false;
  return true;
}

const MIME_BY_EXT: Record<string, string> = {
  txt: "text/plain", md: "text/plain", csv: "text/plain", log: "text/plain",
  html: "text/html", htm: "text/html", json: "application/json", pdf: "application/pdf",
  zip: "application/zip", png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg",
  gif: "image/gif", webp: "image/webp", svg: "image/svg+xml", mp3: "audio/mpeg",
  wav: "audio/wav", mp4: "video/mp4", doc: "application/msword",
  docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  xls: "application/vnd.ms-excel",
  xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  ppt: "application/vnd.ms-powerpoint",
  pptx: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
};

export function guessMime(filename: string): string {
  const dot = filename.lastIndexOf(".");
  const ext = dot === -1 ? "" : filename.slice(dot + 1).toLowerCase();
  return MIME_BY_EXT[ext] ?? "application/octet-stream";
}

export function buildSendRaw(
  to: string[],
  subject: string,
  body: string,
  attachments: SendAttachment[],
): string {
  if (to.length === 0) throw new Error("add a recipient before sending");
  const bodyBytes = Buffer.byteLength(body, "utf8");
  const attachmentBytes = attachments.reduce((sum, a) => sum + a.bytes.length, 0);
  if (bodyBytes + attachmentBytes > MAX_SEND_BYTES) {
    throw new Error("the message is over Gmail's 25 MB limit");
  }
  const boundary = makeBoundary();
  let message = `To: ${to.join(", ")}\r\nSubject: ${encodeSubject(subject)}\r\nMIME-Version: 1.0\r\n`;
  if (attachments.length === 0) {
    message +=
      `Content-Type: text/plain; charset="utf-8"\r\nContent-Transfer-Encoding: base64\r\n\r\n` +
      `${wrapBase64(Buffer.from(body, "utf8").toString("base64"))}\r\n`;
  } else {
    message += `Content-Type: multipart/mixed; boundary="${boundary}"\r\n\r\n`;
    message +=
      `--${boundary}\r\nContent-Type: text/plain; charset="utf-8"\r\n` +
      `Content-Transfer-Encoding: base64\r\n\r\n` +
      `${wrapBase64(Buffer.from(body, "utf8").toString("base64"))}\r\n`;
    for (const attachment of attachments) {
      const name = sanitizeFilename(attachment.filename);
      const mime = attachment.mimeType || "application/octet-stream";
      message +=
        `--${boundary}\r\nContent-Type: ${mime}; name="${name}"\r\n` +
        `Content-Disposition: attachment; filename="${name}"\r\n` +
        `Content-Transfer-Encoding: base64\r\n\r\n` +
        `${wrapBase64(Buffer.from(attachment.bytes).toString("base64"))}\r\n`;
    }
    message += `--${boundary}--\r\n`;
  }
  return Buffer.from(message, "utf8").toString("base64url");
}

function makeBoundary(): string {
  return `----nori-${randomBytes(16).toString("hex")}`;
}

function wrapBase64(encoded: string): string {
  const chunks: string[] = [];
  for (let i = 0; i < encoded.length; i += 76) chunks.push(encoded.slice(i, i + 76));
  return chunks.join("\r\n");
}

function encodeSubject(subject: string): string {
  const collapsed = subject.replace(/[\r\n]+/g, " ").split(/\s+/).join(" ").trim();
  if (!collapsed || /^[\x00-\x7f]*$/.test(collapsed)) return collapsed;
  return `=?UTF-8?B?${Buffer.from(collapsed, "utf8").toString("base64")}?=`;;
}

function sanitizeFilename(filename: string): string {
  const base = filename.split(/[/\\]/).pop() ?? filename;
  const cleaned = base.replace(/["\r\n]/g, "").trim();
  return cleaned || "attachment";
}
