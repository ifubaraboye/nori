// Port of crates/nori-gmail/src/sanitize.rs — sanitize-once-at-ingest.
//
// Rust uses ammonia with an allowlist that keeps table layout + cid:/data:
// images for later rewriting. This port implements the same allowlist with a
// dependency-free tokenizer so the Electron main process stays lean.
const ALLOWED_TAGS = new Set([
  "html", "head", "body", "meta", "title", "table", "thead", "tbody", "tfoot",
  "tr", "td", "th", "center", "font", "img", "style", "p", "div", "span",
  "br", "hr", "a", "b", "strong", "i", "em", "u", "ul", "ol", "li",
  "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre", "code",
  "section", "article", "header", "footer", "main", "figure", "figcaption",
]);

const GENERIC_ATTRS = new Set([
  "style", "class", "id", "align", "valign", "width", "height",
  "border", "cellpadding", "cellspacing", "bgcolor", "color",
]);

const TAG_ATTRS: Record<string, Set<string>> = {
  meta: new Set(["name", "content", "http-equiv"]),
  img: new Set(["src", "alt", "width", "height"]),
  font: new Set(["face", "size", "color"]),
  a: new Set(["href", "title"]),
};

const URL_SCHEMES = ["http://", "https://", "mailto:", "tel:", "cid:", "data:"];
const VOID_ELEMENTS = new Set([
  "area", "base", "br", "col", "embed", "hr", "img", "input",
  "link", "meta", "param", "source", "track", "wbr",
]);

export function sanitizeHtml(html: string): string {
  const head = htmlDocumentSection(html, "head");
  const body = htmlDocumentSection(html, "body");
  if (!head && !body) return cleanFragment(html);
  const headContent = head ? cleanFragment(head.content) : "";
  const bodyContent = body ? cleanFragment(body.content) : cleanFragment(html);
  const htmlAttrs = sanitizeContainerAttributes(htmlDocumentSection(html, "html")?.attributes ?? "");
  const headAttrs = sanitizeContainerAttributes(head?.attributes ?? "");
  const bodyAttrs = sanitizeContainerAttributes(body?.attributes ?? "");
  return `<html${htmlAttrs}><head${headAttrs}>${headContent}</head><body${bodyAttrs}>${bodyContent}</body></html>`;
}

interface Section {
  attributes: string;
  content: string;
}

function htmlDocumentSection(html: string, tag: string): Section | null {
  const lower = html.toLowerCase();
  const open = findHtmlTag(lower, tag, 0, false);
  if (open === -1) return null;
  const openEnd = htmlTagEnd(html, open);
  if (openEnd === -1) return null;
  const tagEnd = html.indexOf(">", open);
  const attributes = tagEnd === -1 ? "" : html.slice(open + tag.length + 1, openEnd);
  const close = findHtmlTag(lower, tag, openEnd + 1, true);
  const content = close === -1 ? html.slice(openEnd + 1) : html.slice(openEnd + 1, close);
  return { attributes, content };
}

function findHtmlTag(lower: string, tag: string, from: number, closing: boolean): number {
  const needle = closing ? `</${tag}` : `<${tag}`;
  let index = lower.indexOf(needle, from);
  while (index !== -1) {
    const after = lower[index + needle.length] ?? "";
    if (after === "" || after === ">" || after === "/" || /\s/.test(after)) return index;
    index = lower.indexOf(needle, index + 1);
  }
  return -1;
}

function htmlTagEnd(html: string, start: number): number {
  let quote: string | null = null;
  for (let i = start; i < html.length; i++) {
    const ch = html[i];
    if (quote) {
      if (ch === quote) quote = null;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
    } else if (ch === ">") {
      return i;
    }
  }
  return -1;
}

function sanitizeContainerAttributes(attributes: string): string {
  const cleaned = cleanFragment(`<div${attributes}></div>`);
  const match = /^<div([^>]*)>/.exec(cleaned);
  return match ? match[1] : "";
}

function cleanFragment(html: string): string {
  let out = "";
  let i = 0;
  let skipTag: string | null = null;
  while (i < html.length) {
    const lt = html.indexOf("<", i);
    if (lt === -1) {
      if (!skipTag) out += escapeText(html.slice(i));
      break;
    }
    if (!skipTag) out += escapeText(html.slice(i, lt));
    const gt = htmlTagEnd(html, lt);
    if (gt === -1) {
      if (!skipTag) out += escapeText(html.slice(lt));
      break;
    }
    const raw = html.slice(lt + 1, gt).trim();
    i = gt + 1;
    if (!raw || raw.startsWith("!") || raw.startsWith("?")) continue;
    const isClose = raw.startsWith("/");
    const name = (isClose ? raw.slice(1) : raw).split(/[\s/]/, 1)[0].toLowerCase();
    if (skipTag) {
      if (isClose && name === skipTag) skipTag = null;
      continue;
    }
    if (name === "script") {
      if (!isClose) skipTag = "script";
      continue;
    }
    if (!ALLOWED_TAGS.has(name)) continue;
    if (isClose) {
      out += `</${name}>`;
      continue;
    }
    const attrs = parseAttributes(raw.slice(name.length));
    const kept = filterAttributes(name, attrs);
    out += VOID_ELEMENTS.has(name) ? `<${name}${kept}>` : `<${name}${kept}>`;
  }
  return out;
}

function parseAttributes(source: string): Array<[string, string]> {
  const attrs: Array<[string, string]> = [];
  const re = /([a-zA-Z_:][a-zA-Z0-9:._-]*)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'`=<>]+)))?/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(source)) !== null) {
    const name = match[1].toLowerCase();
    const value = match[2] ?? match[3] ?? match[4] ?? "";
    attrs.push([name, value]);
  }
  return attrs;
}

function filterAttributes(tag: string, attrs: Array<[string, string]>): string {
  let out = "";
  const extra = TAG_ATTRS[tag];
  for (const [name, value] of attrs) {
    if (GENERIC_ATTRS.has(name) || extra?.has(name)) {
      if ((name === "href" || name === "src") && !allowedUrl(value)) continue;
      out += ` ${name}="${escapeAttr(value)}"`;
    }
  }
  if (tag === "a" && !attrs.some(([n]) => n === "rel")) {
    out += ` rel="noopener noreferrer"`;
  }
  return out;
}

function allowedUrl(value: string): boolean {
  const lower = value.trim().toLowerCase();
  return URL_SCHEMES.some((scheme) => lower.startsWith(scheme));
}

function escapeText(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function escapeAttr(value: string): string {
  return value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");
}
