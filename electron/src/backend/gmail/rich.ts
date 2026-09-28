// Port of crates/nori-gmail/src/rich.rs — sanitized HTML -> RichBody blocks.
//
// Budgets mirror Rust: MAX_BLOCKS 2000, table depth 8, 2000 cells,
// 8MiB/image + 16MiB/message + 32 images for cid: inlining, 12MiB data: URLs,
// trackers (1x1) dropped, only https:/data: image sources survive.

import { sanitizeHtml } from "./sanitize.js";
import { boundedHtml } from "./policy.js";

export interface CssColor {
  r: number;
  g: number;
  b: number;
  a: number;
}

export interface RichSpan {
  text: string;
  bold: boolean;
  italic: boolean;
  link?: string;
  color?: CssColor;
  background?: CssColor;
  fontSize?: number;
  align?: "left" | "center" | "right" | "justify";
}

export type RichBlock =
  | { kind: "paragraph"; spans: RichSpan[] }
  | { kind: "heading"; level: number; spans: RichSpan[] }
  | { kind: "list"; ordered: boolean; items: RichSpan[][] }
  | { kind: "quote"; blocks: RichBlock[] }
  | { kind: "rule" }
  | { kind: "image"; src: string; alt: string; link?: string; width?: number; height?: number }
  | { kind: "grid"; rows: RichBlock[][][] };

export type RichBody = RichBlock[];

const MAX_BLOCKS = 2000;
const MAX_INLINE_CID_BYTES_PER_IMAGE = 8 * 1024 * 1024;
const MAX_INLINE_CID_BYTES_PER_MESSAGE = 16 * 1024 * 1024;
const MAX_INLINE_CID_IMAGES = 32;
const MAX_DEPTH = 8;
const MAX_CELLS = 2000;
const BASE_FONT_SIZE = 14;
const MAX_INLINE_DATA_URL_LEN = 12 * 1024 * 1024;

export function plainSpan(text: string): RichSpan {
  return { text, bold: false, italic: false };
}

export function sameRun(a: RichSpan, b: RichSpan): boolean {
  return (
    a.bold === b.bold &&
    a.italic === b.italic &&
    (a.link ?? null) === (b.link ?? null) &&
    JSON.stringify(a.color ?? null) === JSON.stringify(b.color ?? null) &&
    JSON.stringify(a.background ?? null) === JSON.stringify(b.background ?? null) &&
    (a.fontSize ?? null) === (b.fontSize ?? null) &&
    (a.align ?? null) === (b.align ?? null)
  );
}

export function textBlocks(paragraphs: string[]): RichBody {
  return paragraphs.map((text) => ({ kind: "paragraph", spans: [plainSpan(text)] }) as RichBlock);
}

export function plainText(blocks: RichBlock[]): string {
  const lines: string[] = [];
  const spanText = (spans: RichSpan[]): string =>
    spans
      .map((s) => (s.link && s.link !== s.text ? `${s.text} <${s.link}>` : s.text))
      .join("");
  const pushBlocks = (items: RichBlock[]) => {
    for (const block of items) {
      switch (block.kind) {
        case "paragraph":
        case "heading":
          lines.push(spanText(block.spans));
          break;
        case "list":
          block.items.forEach((item, i) =>
            lines.push(`${block.ordered ? `${i + 1}. ` : "• "}${spanText(item)}`),
          );
          break;
        case "quote":
          for (const line of plainText(block.blocks).split("\n")) lines.push(`> ${line}`);
          break;
        case "rule":
          lines.push("---");
          break;
        case "image":
          lines.push(block.alt ? `[image: ${block.alt}]` : "[image]");
          break;
        case "grid":
          for (const row of block.rows) {
            lines.push(
              row.map((cell) => plainText(cell).split("\n").join(" ")).join("\t"),
            );
          }
          break;
      }
    }
  };
  pushBlocks(blocks);
  return lines.join("\n\n");
}

export function imageSources(blocks: RichBlock[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  const visit = (items: RichBlock[]) => {
    for (const block of items) {
      if (block.kind === "image") {
        if (!seen.has(block.src)) {
          seen.add(block.src);
          out.push(block.src);
        }
      } else if (block.kind === "quote") {
        visit(block.blocks);
      } else if (block.kind === "grid") {
        for (const row of block.rows) for (const cell of row) visit(cell);
      }
    }
  };
  visit(blocks);
  return out;
}

export function inlineCidImages(html: string, images: Array<[string, string, Uint8Array]>): string {
  if (!html.includes("cid:") || images.length === 0) return html;
  const wanted = referencedCids(html);
  if (wanted.size === 0) return html;
  const map = new Map<string, string>();
  let budget = MAX_INLINE_CID_BYTES_PER_MESSAGE;
  let count = 0;
  for (const [id, mime, bytes] of images) {
    const normalized = normalizeCid(id);
    if (!wanted.has(normalized)) continue;
    if (!mime.startsWith("image/") || bytes.length === 0) continue;
    if (bytes.length > MAX_INLINE_CID_BYTES_PER_IMAGE || bytes.length > budget) continue;
    if (count >= MAX_INLINE_CID_IMAGES) break;
    map.set(normalized, `data:${mime};base64,${Buffer.from(bytes).toString("base64")}`);
    budget -= bytes.length;
    count++;
  }
  if (map.size === 0) return html;
  return rewriteCidSrc(html, map);
}

function normalizeCid(id: string): string {
  return id.trim().replace(/^<+/, "").replace(/>+$/, "");
}

function referencedCids(html: string): Set<string> {
  const out = new Set<string>();
  const re = /src\s*=\s*("([^"]*)"|'([^']*)')/gi;
  let match: RegExpExecArray | null;
  while ((match = re.exec(html)) !== null) {
    const value = match[2] ?? match[3] ?? "";
    const cidIndex = value.indexOf("cid:");
    if (cidIndex === -1) continue;
    out.add(normalizeCid(value.slice(cidIndex + 4)));
  }
  return out;
}

function rewriteCidSrc(html: string, map: Map<string, string>): string {
  return html.replace(/src\s*=\s*("([^"]*)"|'([^']*)')/gi, (full, _quoted, dq, sq) => {
    const value: string = dq ?? sq ?? "";
    const quote = full.includes('"') ? '"' : "'";
    const cidIndex = value.indexOf("cid:");
    if (cidIndex === -1) return full;
    const replacement = map.get(normalizeCid(value.slice(cidIndex + 4)));
    if (!replacement) return full;
    return `src=${quote}${replacement}${quote}`;
  });
}

interface InlineStyle {
  bold: boolean;
  italic: boolean;
  link?: string;
  color?: CssColor;
  background?: CssColor;
  fontSize?: number;
  align?: RichSpan["align"];
  verbatim?: boolean;
}

interface DomNode {
  tag: string;
  attrs: Record<string, string>;
  children: DomNode[];
  text?: string;
}

export function parseHtmlBody(html: string): RichBody {
  const { html: bounded, notice } = boundedHtml(sanitizeHtml(html));
  if (notice && !bounded) return [];
  if (notice && bounded && !bounded.includes("<")) {
    return textBlocks(bounded.split("\n").map((l) => l.trim()).filter(Boolean));
  }
  const root = parseDom(bounded || html);
  const walker = new Walker(0);
  walker.walkChildren(root, { bold: false, italic: false });
  walker.flushPara();
  return walker.out.slice(0, MAX_BLOCKS);
}

class Walker {
  out: RichBlock[] = [];
  para: RichSpan[] = [];
  cells = 0;

  constructor(readonly depth: number) {}

  pushBlock(block: RichBlock): void {
    if (this.out.length >= MAX_BLOCKS) return;
    this.out.push(block);
  }

  flushPara(): void {
    const spans = trimSpans(this.para);
    this.para = [];
    if (spans.length === 0) return;
    this.pushBlock({ kind: "paragraph", spans });
  }

  pushText(text: string, style: InlineStyle): void {
    const collapsed = style.verbatim ? text : collapseWs(text);
    if (!collapsed) return;
    const span: RichSpan = {
      text: collapsed,
      bold: style.bold,
      italic: style.italic,
      link: style.link,
      color: style.color,
      background: style.background,
      fontSize: style.fontSize,
      align: style.align,
    };
    const last = this.para[this.para.length - 1];
    if (last && sameRun(last, span)) {
      last.text += (/[\s]$/.test(last.text) || /^[\s]/.test(span.text) ? "" : " ") + span.text;
    } else {
      if (this.para.length > 0 && !/[\s]$/.test(this.para[this.para.length - 1].text) && /^[\s]/.test(span.text)) {
        span.text = span.text.replace(/^\s+/, " ");
      }
      this.para.push(span);
    }
  }

  walkChildren(node: DomNode, style: InlineStyle): void {
    for (const child of node.children) this.walkNode(child, style);
  }

  walkNode(node: DomNode, parent: InlineStyle): void {
    if (node.tag === "#text") {
      this.pushText(node.text ?? "", parent);
      return;
    }
    if (node.tag === "br") {
      const last = this.para[this.para.length - 1];
      if (last) last.text += "\n";
      else this.para.push(plainSpan("\n"));
      return;
    }
    if (["script", "style", "head", "title", "meta", "link", "noscript", "template", "svg", "video", "audio", "canvas", "input", "select", "textarea", "iframe", "object", "embed"].includes(node.tag)) {
      return;
    }
    if (isHidden(node)) return;
    const style: InlineStyle = {
      bold: parent.bold || node.tag === "b" || node.tag === "strong",
      italic: parent.italic || node.tag === "i" || node.tag === "em",
      link: node.tag === "a" ? httpsUrl(node.attrs.href) ?? parent.link : parent.link,
      ...inheritTextStyle(parseStyleAttr(node.attrs.style), parent),
      verbatim: parent.verbatim || node.tag === "pre",
    };
    switch (node.tag) {
      case "p":
      case "div":
      case "section":
      case "article":
      case "header":
      case "footer":
      case "main":
      case "center":
      case "figure":
      case "figcaption":
      case "pre":
        this.flushPara();
        this.walkChildren(node, style);
        this.flushPara();
        return;
      case "hr":
        this.flushPara();
        this.pushBlock({ kind: "rule" });
        return;
      case "h1": case "h2": case "h3": case "h4": case "h5": case "h6": {
        this.flushPara();
        const sub = new Walker(this.depth);
        sub.walkChildren(node, style);
        sub.flushPara();
        const spans = sub.out.flatMap((b) => (b.kind === "paragraph" ? b.spans : []));
        if (spans.length > 0) {
          this.pushBlock({ kind: "heading", level: Math.min(3, Number(node.tag[1])), spans });
        }
        return;
      }
      case "ul":
      case "ol": {
        this.flushPara();
        const items: RichSpan[][] = [];
        for (const child of node.children) {
          if (child.tag === "li") {
            const sub = new Walker(this.depth);
            sub.walkChildren(child, style);
            sub.flushPara();
            const spans = sub.out.flatMap((b) => (b.kind === "paragraph" ? b.spans : []));
            if (spans.length > 0) items.push(spans);
          }
        }
        if (items.length > 0) this.pushBlock({ kind: "list", ordered: node.tag === "ol", items });
        return;
      }
      case "blockquote": {
        this.flushPara();
        const sub = new Walker(this.depth);
        sub.walkChildren(node, style);
        sub.flushPara();
        if (sub.out.length > 0) this.pushBlock({ kind: "quote", blocks: sub.out });
        return;
      }
      case "img": {
        const image = parseImage(node.attrs, style.link);
        if (image) {
          this.flushPara();
          this.pushBlock(image);
        }
        return;
      }
      case "table": {
        this.flushPara();
        const grid = this.walkGrid(node, style);
        if (grid) this.pushBlock(grid);
        return;
      }
      default:
        this.walkChildren(node, style);
    }
  }

  walkGrid(table: DomNode, style: InlineStyle): RichBlock | null {
    if (this.depth >= MAX_DEPTH) {
      const sub = new Walker(this.depth);
      sub.walkChildren(table, style);
      sub.flushPara();
      for (const block of sub.out) this.pushBlock(block);
      return null;
    }
    const rows: RichBlock[][][] = [];
    const rowElements = collectRows(table);
    for (const row of rowElements) {
      const cells: RichBlock[][] = [];
      for (const cell of row) {
        if (this.cells >= MAX_CELLS) break;
        this.cells++;
        const sub = new Walker(this.depth + 1);
        sub.walkChildren(cell, style);
        sub.flushPara();
        cells.push(sub.out);
      }
      if (cells.length > 0) rows.push(cells);
    }
    if (rows.length === 0) return null;
    if (rows.flat(2).length <= 1) {
      for (const row of rows) for (const cell of row) for (const block of cell) this.pushBlock(block);
      return null;
    }
    return { kind: "grid", rows };
  }
}

function collectRows(table: DomNode): DomNode[][] {
  const rows: DomNode[][] = [];
  const walk = (node: DomNode, current: DomNode[] | null) => {
    if (node.tag === "tr") {
      const cells = node.children.filter((c) => c.tag === "td" || c.tag === "th");
      rows.push(cells);
      return;
    }
    if (node.tag === "td" || node.tag === "th") {
      if (!current) rows.push([node]);
      else current.push(node);
      return;
    }
    if (["table", "thead", "tbody", "tfoot"].includes(node.tag)) {
      for (const child of node.children) walk(child, null);
    }
  };
  for (const child of table.children) {
    if (child.tag === "tr") {
      rows.push(child.children.filter((c) => c.tag === "td" || c.tag === "th"));
    } else {
      walk(child, null);
    }
  }
  return rows.filter((r) => r.length > 0);
}

function trimSpans(spans: RichSpan[]): RichSpan[] {
  const out = spans.filter((s) => s.text.trim() !== "" || spans.length === 1);
  if (out.length === 0) return out;
  out[0] = { ...out[0], text: out[0].text.replace(/^\s+/, "") };
  out[out.length - 1] = { ...out[out.length - 1], text: out[out.length - 1].text.replace(/\s+$/, "") };
  return out.filter((s) => s.text !== "");
}

function collapseWs(text: string): string {
  return text.replace(/[\t\n\f\r ]+/g, " ");
}

function isHidden(node: DomNode): boolean {
  if ("hidden" in node.attrs) return true;
  const style = (node.attrs.style ?? "").replace(/\s+/g, "").toLowerCase();
  return style.includes("display:none");
}

function httpsUrl(href: string | undefined): string | undefined {
  if (!href) return undefined;
  const trimmed = href.trim();
  return trimmed.startsWith("https://") && trimmed.length > 8 ? trimmed : undefined;
}

function parseImage(attrs: Record<string, string>, link: string | undefined): RichBlock | null {
  const src = (attrs.src ?? "").trim();
  if (!src) return null;
  let resolved: string | undefined;
  if (src.startsWith("data:image/")) {
    if (src.length > MAX_INLINE_DATA_URL_LEN) return null;
    resolved = src;
  } else {
    resolved = httpsUrl(src);
    if (!resolved) return null;
  }
  const width = parseLengthPx(attrs.width ?? "");
  const height = parseLengthPx(attrs.height ?? "");
  if (width === 1 && height === 1) return null;
  const alt = decodeEntities(attrs.alt ?? "").trim();
  return { kind: "image", src: resolved, alt, link, width: width ?? undefined, height: height ?? undefined };
}

function parseLengthPx(value: string): number | null {
  const cleaned = value.split("!")[0].trim().toLowerCase();
  const match = /^(-?[\d.]+)\s*(px|pt)?$/.exec(cleaned);
  if (!match) return null;
  let num = Number(match[1]);
  if (!Number.isFinite(num) || num <= 0) return null;
  if (match[2] === "pt") num = (num * 4) / 3;
  return Math.min(10000, Math.max(1, Math.round(num)));
}

function inheritTextStyle(
  child: { color?: CssColor; background?: CssColor; fontSize?: number; align?: RichSpan["align"] },
  parent: InlineStyle,
): Pick<InlineStyle, "color" | "background" | "fontSize" | "align"> {
  return {
    color: child.color ?? parent.color,
    background: child.background ?? parent.background,
    fontSize: child.fontSize ?? parent.fontSize,
    align: child.align ?? parent.align,
  };
}

function parseStyleAttr(style: string | undefined): { color?: CssColor; background?: CssColor; fontSize?: number; align?: RichSpan["align"] } {
  const out: { color?: CssColor; background?: CssColor; fontSize?: number; align?: RichSpan["align"] } = {};
  if (!style) return out;
  for (const decl of style.split(";")) {
    const colon = decl.indexOf(":");
    if (colon === -1) continue;
    const name = decl.slice(0, colon).trim().toLowerCase();
    const value = decl.slice(colon + 1).split("!")[0].trim();
    if (name === "color") out.color = parseCssColor(value);
    else if (name === "background" || name === "background-color") out.background = parseCssColor(value);
    else if (name === "font-size") out.fontSize = parseFontSize(value) ?? undefined;
    else if (name === "text-align") {
      if (value === "left" || value === "start") out.align = "left";
      else if (value === "center" || value === "centre") out.align = "center";
      else if (value === "right" || value === "end") out.align = "right";
      else if (value === "justify") out.align = "justify";
    }
  }
  return out;
}

function parseCssColor(value: string): CssColor | undefined {
  const lower = value.trim().toLowerCase();
  if (!lower || lower === "transparent") return { r: 0, g: 0, b: 0, a: 0 };
  if (lower.startsWith("#")) return parseHexColor(lower);
  const rgb = /^rgba?\((.*)\)$/.exec(lower);
  if (rgb) {
    const parts = rgb[1].split(",").map((p) => p.trim());
    if (parts.length < 3) return undefined;
    const channels = parts.slice(0, 3).map((p) => {
      if (p.endsWith("%")) return Math.round((Number(p.slice(0, -1)) / 100) * 255);
      return Number(p);
    });
    if (channels.some((c) => !Number.isFinite(c))) return undefined;
    let alpha = 255;
    if (parts[3] !== undefined) {
      const a = parts[3].endsWith("%") ? Number(parts[3].slice(0, -1)) / 100 : Number(parts[3]);
      alpha = a <= 1 ? Math.round(a * 255) : Math.round(a);
    }
    return { r: clamp255(channels[0]), g: clamp255(channels[1]), b: clamp255(channels[2]), a: clamp255(alpha) };
  }
  return namedColor(lower);
}

function clamp255(n: number): number {
  return Math.min(255, Math.max(0, Math.round(n)));
}

function parseHexColor(lower: string): CssColor | undefined {
  const hex = lower.slice(1);
  const pair = (s: string): number => parseInt(s, 16);
  if (hex.length === 3 || hex.length === 4) {
    const [r, g, b, a] = hex.split("").map((c) => pair(c + c));
    return { r, g, b, a: hex.length === 4 ? a : 255 };
  }
  if (hex.length === 6 || hex.length === 8) {
    const r = pair(hex.slice(0, 2));
    const g = pair(hex.slice(2, 4));
    const b = pair(hex.slice(4, 6));
    const a = hex.length === 8 ? pair(hex.slice(6, 8)) : 255;
    if ([r, g, b, a].some((c) => Number.isNaN(c))) return undefined;
    return { r, g, b, a };
  }
  return undefined;
}

const NAMED_COLORS: Record<string, [number, number, number]> = {
  black: [0, 0, 0], white: [255, 255, 255], red: [255, 0, 0], green: [0, 128, 0],
  blue: [0, 0, 255], gray: [128, 128, 128], grey: [128, 128, 128], crimson: [220, 20, 60],
  navy: [0, 0, 128], teal: [0, 128, 128], orange: [255, 165, 0], purple: [128, 0, 128],
  yellow: [255, 255, 0],
};

function namedColor(name: string): CssColor | undefined {
  const rgb = NAMED_COLORS[name];
  if (!rgb) return undefined;
  return { r: rgb[0], g: rgb[1], b: rgb[2], a: 255 };
}

function parseFontSize(value: string): number | null {
  const lower = value.trim().toLowerCase();
  const keywords: Record<string, number> = {
    "xx-small": 9, "x-small": 10, small: 13, medium: 14, large: 18, "x-large": 24, "xx-large": 32,
  };
  if (keywords[lower] !== undefined) return keywords[lower];
  const match = /^(-?[\d.]+)\s*(px|pt|em|rem|%)?$/.exec(lower);
  if (!match) return null;
  const num = Number(match[1]);
  if (!Number.isFinite(num) || num <= 0) return null;
  const unit = match[2] ?? "px";
  let px = num;
  if (unit === "pt") px = (num * 4) / 3;
  else if (unit === "em" || unit === "rem") px = num * BASE_FONT_SIZE;
  else if (unit === "%") px = (num * BASE_FONT_SIZE) / 100;
  const rounded = Math.round(px);
  if (rounded < 1 || rounded > 200) return Math.min(200, Math.max(1, rounded));
  return rounded;
}

function decodeEntities(text: string): string {
  return text
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'");
}

// Minimal dependency-free HTML parser sufficient for sanitized mail bodies.
function parseDom(html: string): DomNode {
  const root: DomNode = { tag: "root", attrs: {}, children: [] };
  const stack: DomNode[] = [root];
  const tagRe = /<!--[\s\S]*?-->|<\/?[a-zA-Z][^>]*>|<[^>]*>/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  const pushText = (text: string) => {
    if (!text) return;
    stack[stack.length - 1].children.push({ tag: "#text", attrs: {}, children: [], text });
  };
  while ((match = tagRe.exec(html)) !== null) {
    pushText(html.slice(lastIndex, match.index));
    lastIndex = tagRe.lastIndex;
    const token = match[0];
    if (token.startsWith("<!--")) continue;
    const inner = token.slice(1, -1).trim();
    const isClose = inner.startsWith("/");
    const name = (isClose ? inner.slice(1) : inner).split(/[\s/]/, 1)[0].toLowerCase();
    if (!name || name.startsWith("!") || name.startsWith("?")) continue;
    if (isClose) {
      while (stack.length > 1 && stack[stack.length - 1].tag !== name) stack.pop();
      if (stack.length > 1) stack.pop();
      continue;
    }
    const attrs = parseTagAttrs(inner.slice(name.length));
    const node: DomNode = { tag: name, attrs, children: [] };
    stack[stack.length - 1].children.push(node);
    if (!["br", "hr", "img", "meta", "link"].includes(name)) stack.push(node);
  }
  pushText(html.slice(lastIndex));
  return root;
}

function parseTagAttrs(source: string): Record<string, string> {
  const attrs: Record<string, string> = {};
  const re = /([a-zA-Z_:][a-zA-Z0-9:._-]*)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'`=<>]+)))?/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(source)) !== null) {
    attrs[match[1].toLowerCase()] = match[2] ?? match[3] ?? match[4] ?? "";
  }
  return attrs;
}
