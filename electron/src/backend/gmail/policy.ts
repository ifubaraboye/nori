// Port of crates/nori-gmail/src/policy.rs — pre-layout budgets over raw HTML.
export const MAX_HTML_BYTES = 24 * 1024 * 1024;
const MAX_TAGS = 20_000;
const MAX_DEPTH = 96;
const MAX_TEXT_BYTES = 1_000_000;

export function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

const VOID_ELEMENTS = new Set([
  "area", "base", "br", "col", "embed", "hr", "img", "input",
  "link", "meta", "param", "source", "track", "wbr",
]);

interface Scan {
  depth: number;
  count: number;
  excessive: boolean;
  text: string;
  remote: boolean;
  truncated: boolean;
}

function scan(html: string): Scan {
  const state: Scan = { depth: 0, count: 0, excessive: false, text: "", remote: false, truncated: false };
  const stack: string[] = [];
  let raw: string | null = null;
  const tagRe = /<!--[\s\S]*?-->|<\/?[a-zA-Z][^>]*>|<[^>]*>/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  const appendText = (text: string) => {
    const remaining = MAX_TEXT_BYTES - Buffer.byteLength(state.text, "utf8");
    if (remaining <= 0) {
      state.truncated = true;
      state.excessive = true;
      return;
    }
    const buf = Buffer.from(text, "utf8");
    if (buf.length > remaining) {
      let end = remaining;
      while (end > 0 && (buf[end] & 0xc0) === 0x80) end--;
      state.text += buf.slice(0, end).toString("utf8");
      state.truncated = true;
      state.excessive = true;
    } else {
      state.text += text;
    }
  };

  const handleChunk = (chunk: string) => {
    if (raw) {
      const tag = raw;
      if (tag === "style") {
        if (cssRemote(chunk)) state.remote = true;
        if (excessiveCss(chunk)) state.excessive = true;
      }
      return;
    }
    appendText(chunk);
  };

  while ((match = tagRe.exec(html)) !== null) {
    handleChunk(html.slice(lastIndex, match.index));
    lastIndex = tagRe.lastIndex;
    const token = match[0];
    if (token.startsWith("<!--")) continue;
    const inner = token.slice(1, -1).trim();
    const isClose = inner.startsWith("/");
    const name = (isClose ? inner.slice(1) : inner).split(/[\s/]/, 1)[0].toLowerCase();
    if (!name) continue;

    if (isClose) {
      const pos = stack.lastIndexOf(name);
      if (pos !== -1) stack.length = pos;
      if (raw && name === raw) raw = null;
      if (["p", "div", "tr", "li", "pre", "h1", "h2", "h3"].includes(name)) appendText("\n");
      continue;
    }

    state.count++;
    if (state.count > MAX_TAGS || html.length > MAX_HTML_BYTES) state.excessive = true;
    if (name === "br") {
      appendText("\n");
      continue;
    }
    if (name === "img") {
      const src = attrValue(inner, "src");
      if (src && isRemote(src)) state.remote = true;
      const alt = attrValue(inner, "alt");
      if (alt) appendText(alt);
    }
    if (["script", "style", "title"].includes(name)) raw = name;
    const span = attrValue(inner, "colspan") || attrValue(inner, "rowspan") || attrValue(inner, "span");
    if (span && Number(span) > 512) state.excessive = true;
    const style = attrValue(inner, "style");
    if (style && excessiveCss(style)) state.excessive = true;
    // Any tag's inline style can pull remote content via url(...).
    if (style && cssRemote(style)) state.remote = true;
    if (VOID_ELEMENTS.has(name)) continue;
    if (state.depth >= MAX_DEPTH) {
      state.excessive = true;
      continue;
    }
    stack.push(name);
    state.depth = Math.max(state.depth, stack.length);
  }
  handleChunk(html.slice(lastIndex));
  return state;
}

function attrValue(tagInner: string, name: string): string {
  const re = new RegExp(`${name}\\s*=\\s*(?:"([^"]*)"|'([^']*)'|([^\\s>]+))`, "i");
  const match = re.exec(tagInner);
  return match ? (match[1] ?? match[2] ?? match[3] ?? "") : "";
}

function excessiveCss(css: string): boolean {
  if (css.length > 512 * 1024) return true;
  // Huge numerics / deep nesting signal a CSS bomb.
  let depth = 0;
  let maxDepth = 0;
  for (const ch of css) {
    if (ch === "(" || ch === "{") {
      depth++;
      maxDepth = Math.max(maxDepth, depth);
    } else if (ch === ")" || ch === "}") {
      depth = Math.max(0, depth - 1);
    }
  }
  if (maxDepth > 64) return true;
  const numbers = css.match(/-?\d+(?:\.\d+)?(?:e[+-]?\d+)?/gi) ?? [];
  for (const n of numbers) {
    const value = Number(n);
    if (!Number.isFinite(value) || Math.abs(value) >= 1e8) return true;
  }
  const repeat = /repeat\s*\(\s*(\d+)/i.exec(css);
  if (repeat && Number(repeat[1]) > 512) return true;
  return false;
}

function readableFallback(text: string): string {
  const lines = text.trim().split("\n");
  const out: string[] = [];
  let blank = false;
  for (const line of lines) {
    if (line.trim() === "") {
      if (!blank) out.push("");
      blank = true;
    } else {
      out.push(line.trimEnd());
      blank = false;
    }
  }
  return out.join("\n").trimEnd();
}

function isRemote(value: string): boolean {
  const lower = value.trim().toLowerCase();
  return lower.startsWith("https:") || lower.startsWith("http:") || lower.startsWith("//");
}

function cssRemote(css: string): boolean {
  const lower = css.toLowerCase();
  return (
    lower.includes("url(") &&
    (lower.includes("http:") || lower.includes("https:") || lower.includes("//"))
  );
}

export function hasRemoteImages(html: string): boolean {
  const capped = html.length > MAX_HTML_BYTES ? html.slice(0, MAX_HTML_BYTES) : html;
  return scan(capped).remote;
}

export function boundedHtml(html: string): { html: string; notice?: string } {
  if (Buffer.byteLength(html, "utf8") > MAX_HTML_BYTES) {
    return { html: "", notice: "Message exceeds the HTML size limit." };
  }
  const result = scan(html);
  if (!result.excessive) return { html };
  return {
    html: readableFallback(result.text),
    notice: result.truncated
      ? "Message text exceeds the display limit."
      : "Complex message shown as plain text.",
  };
}

export function fallbackText(html: string): string {
  const capped = html.length > MAX_HTML_BYTES ? html.slice(0, MAX_HTML_BYTES) : html;
  return readableFallback(scan(capped).text);
}
