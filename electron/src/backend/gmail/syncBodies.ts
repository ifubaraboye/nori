// Gmail full-message parsing: bodies + attachments (sync.rs collect_parts et al).
import { inlineCidImages, parseHtmlBody, plainText, textBlocks } from "./rich.js";
import type { AttachmentMeta } from "./sync.js";

interface Part {
  mimeType?: string;
  filename?: string;
  headers?: Array<{ name?: string; value?: string }>;
  body?: { data?: string; attachmentId?: string; size?: number };
  parts?: Part[];
}

function asPart(message: unknown): Part {
  const value = (message ?? {}) as { payload?: Part };
  return value.payload ?? {};
}

function decodeBytes(data: string): Uint8Array | null {
  try {
    const normalized = data.replace(/-/g, "+").replace(/_/g, "/");
    return Uint8Array.from(Buffer.from(normalized, "base64"));
  } catch {
    return null;
  }
}

function decodeBody(data: string): string {
  const bytes = decodeBytes(data);
  if (!bytes) return data;
  try {
    return Buffer.from(bytes).toString("utf8");
  } catch {
    return data;
  }
}

function partHeader(part: Part, name: string): string | undefined {
  const lower = name.toLowerCase();
  return part.headers?.find((h) => (h.name ?? "").toLowerCase() === lower)?.value;
}

function collectParts(
  part: Part | undefined,
  plain: string[],
  html: string[],
  images: Array<[string, string, string]>,
): void {
  if (!part) return;
  const data = part.body?.data;
  const mime = part.mimeType ?? "";
  const isLeaf = Boolean(data) && (mime || part.filename);
  if (isLeaf && data) {
    if (mime.startsWith("image/")) {
      const contentId = partHeader(part, "Content-ID") ?? partHeader(part, "Content-Id") ?? "";
      if (contentId) {
        images.push([contentId, mime, data]);
        return;
      }
    }
    const text = decodeBody(data);
    if (mime === "text/html") html.push(text);
    else if (mime === "text/plain") plain.push(text);
    else if (mime.startsWith("image/")) images.push([part.filename ?? "", mime, data]);
    return;
  }
  for (const child of part.parts ?? []) collectParts(child, plain, html, images);
}

function normalizePlainText(text: string): string {
  return collapseBlankLines(unwrapAngleUrls(text.trim()));
}

function unwrapAngleUrls(text: string): string {
  let out = "";
  let i = 0;
  while (i < text.length) {
    const open = text.indexOf("<", i);
    if (open === -1) {
      out += text.slice(i);
      break;
    }
    out += text.slice(i, open + 1);
    const rest = text.slice(open + 1);
    if (rest.startsWith("http://") || rest.startsWith("https://")) {
      const close = text.indexOf(">", open + 1);
      if (close !== -1) {
        const candidate = text.slice(open + 1, close);
        if (/^https?:\/\/\S+$/.test(candidate)) {
          out = out.slice(0, -1) + candidate;
          i = close + 1;
          continue;
        }
      }
    }
    i = open + 1;
  }
  return out;
}

function collapseBlankLines(text: string): string {
  const lines = text.split("\n");
  const out: string[] = [];
  let blanks = 0;
  for (const line of lines) {
    if (line.trim() === "") {
      blanks++;
      if (blanks <= 2) out.push("");
    } else {
      blanks = 0;
      out.push(line);
    }
  }
  while (out.length > 0 && out[0].trim() === "") out.shift();
  while (out.length > 0 && out[out.length - 1].trim() === "") out.pop();
  return out.join("\n");
}

function htmlToText(html: string): string {
  let out = "";
  let inTag = false;
  let tag = "";
  const flushTag = () => {
    const name = tag.split(/[\s/]/, 1)[0].toLowerCase();
    if (["p", "br", "div", "tr", "li", "h1", "h2", "h3", "blockquote"].includes(name)) out += "\n\n";
    tag = "";
  };
  for (let i = 0; i < html.length; i++) {
    const ch = html[i];
    if (!inTag && ch === "<") {
      inTag = true;
      tag = "";
    } else if (inTag && ch === ">") {
      inTag = false;
      flushTag();
    } else if (inTag) {
      tag += ch;
    } else {
      out += ch;
    }
  }
  return collapseBlankLines(
    out.replace(/&nbsp;/g, " ").replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#39;/g, "'"),
  );
}

export function bodyParagraphs(message: unknown): string[] {
  const root = asPart(message);
  const plain: string[] = [];
  const html: string[] = [];
  collectParts(root, plain, html, []);
  const source = plain.length > 0 ? plain.join("\n") : html.join("\n");
  const text = plain.length > 0 ? normalizePlainText(source) : htmlToText(source);
  return text.split("\n\n").map((p) => p.trim()).filter(Boolean);
}

export function richBodyFromMessage(message: unknown): string[] {
  void message;
  return bodyParagraphs(message);
}

export function attachmentsAndBlocks(message: unknown): {
  blocks: ReturnType<typeof parseHtmlBody>;
  attachments: AttachmentMeta[];
} {
  const root = asPart(message);
  const plain: string[] = [];
  const html: string[] = [];
  const images: Array<[string, string, string]> = [];
  collectParts(root, plain, html, images);

  let blocks = plainText.length
    ? textBlocks(bodyParagraphs(message))
    : textBlocks([]);
  if (html.length > 0) {
    const decodedImages: Array<[string, string, Uint8Array]> = [];
    for (const [id, mime, data] of images) {
      const bytes = decodeBytes(data);
      if (bytes) decodedImages.push([id, mime, bytes]);
    }
    const inlined = inlineCidImages(html.join("\n"), decodedImages);
    blocks = parseHtmlBody(inlined);
    if (blocks.length === 0) blocks = textBlocks(bodyParagraphs(message));
  }

  const attachments: AttachmentMeta[] = [];
  const walk = (part: Part | undefined): void => {
    if (!part) return;
    const mime = part.mimeType ?? "";
    const filename = part.filename ?? "";
    const attachmentId = part.body?.attachmentId ?? "";
    const hasInlineData = Boolean(part.body?.data);
    const isBodyText = mime.startsWith("text/") && !filename;
    if (filename && attachmentId && !isBodyText && !hasInlineData) {
      attachments.push({
        filename,
        mimeType: mime || "application/octet-stream",
        size: part.body?.size ?? 0,
        attachmentId,
      });
    }
    for (const child of part.parts ?? []) walk(child);
  };
  walk(root);
  return { blocks, attachments };
}
