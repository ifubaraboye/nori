//! Structured HTML bodies: paragraphs, headings, emphasis, links, lists,
//! quotes, rules, and images — clean and readable, never pixel-faithful.
//!
//! Marketing mail is layout tables all the way down, so the walker treats
//! tables as transparent containers and stacks their cells: what survives is
//! the reading order, not the grid. There is deliberately no CSS support —
//! not colours, not fonts, not `float` — with one documented exception: a
//! `display:none` style hides its subtree, because ESP preheaders live
//! behind it and would otherwise print as junk paragraphs.
//!
//! Plain data only, no rendering types: this crate cannot see `gpui`, so
//! whatever the walker produces has to cross into `nori-ui` as strings.

use serde::{Deserialize, Serialize};
use tl::{Node, NodeHandle, Parser, ParserOptions};

/// One inline run: text with emphasis flags and an optional link target.
/// A link run's `text` is what the mail showed; `link` is where it goes.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct RichSpan {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub link: Option<String>,
}

impl RichSpan {
    /// Unstyled text, which is all a plain-text paragraph ever holds.
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.to_string(),
            bold: false,
            italic: false,
            link: None,
        }
    }
}

/// One block of a rich body, in reading order.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum RichBlock {
    Paragraph(Vec<RichSpan>),
    Heading {
        level: u8,
        spans: Vec<RichSpan>,
    },
    List {
        ordered: bool,
        items: Vec<Vec<RichSpan>>,
    },
    Quote(Vec<RichBlock>),
    Rule,
    /// A remote image. `src` is always `https`: anything else never leaves
    /// the parser, and 1×1 tracking pixels never leave it either. `link` is
    /// the surrounding anchor when the image itself is the click target.
    Image {
        src: String,
        alt: String,
        link: Option<String>,
    },
}

/// A parsed body: empty when the HTML carried nothing readable.
pub type RichBody = Vec<RichBlock>;

/// Backstop against pathological mail: a 10MB tag soup still walks, but it
/// does not get to build ten thousand blocks.
const MAX_BLOCKS: usize = 2000;

/// Every remote image a body references, in reading order and deduplicated.
/// Quotes count: a quoted mail carries its pictures with it. Callers cap
/// how many they actually fetch.
pub fn image_sources(blocks: &[RichBlock]) -> Vec<String> {
    fn collect(blocks: &[RichBlock], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                RichBlock::Image { src, .. } => {
                    if !out.iter().any(|known| known == src) {
                        out.push(src.clone());
                    }
                }
                RichBlock::Quote(inner) => collect(inner, out),
                _ => {}
            }
        }
    }

    let mut out = Vec::new();
    collect(blocks, &mut out);
    out
}

/// Plain paragraphs become single-span blocks, so every body in the app —
/// synced, sampled, or hand-built — travels as the same type.
pub fn text_blocks(paragraphs: Vec<String>) -> RichBody {
    paragraphs
        .into_iter()
        .map(|paragraph| RichBlock::Paragraph(vec![RichSpan::plain(&paragraph)]))
        .collect()
}

/// Project blocks back to plain text, for reply quotes and anywhere else a
/// string is owed. Links stay visible as `text <url>` so a quote never
/// swallows where its links went.
pub fn plain_text(blocks: &[RichBlock]) -> String {
    fn spans_text(spans: &[RichSpan]) -> String {
        spans
            .iter()
            .map(|span| match &span.link {
                Some(url) if span.text == *url => url.clone(),
                Some(url) => format!("{} <{url}>", span.text),
                None => span.text.clone(),
            })
            .collect::<Vec<_>>()
            .join("")
    }

    fn blocks_text(blocks: &[RichBlock], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                RichBlock::Paragraph(spans) | RichBlock::Heading { spans, .. } => {
                    out.push(spans_text(spans));
                }
                RichBlock::List { ordered, items } => {
                    for (index, item) in items.iter().enumerate() {
                        let marker = if *ordered {
                            format!("{}. ", index + 1)
                        } else {
                            "• ".to_string()
                        };
                        out.push(format!("{marker}{}", spans_text(item)));
                    }
                }
                RichBlock::Quote(inner) => {
                    let mut quoted = Vec::new();
                    blocks_text(inner, &mut quoted);
                    out.extend(quoted.into_iter().map(|line| format!("> {line}")));
                }
                RichBlock::Rule => out.push("---".to_string()),
                RichBlock::Image { alt, .. } => {
                    out.push(if alt.is_empty() {
                        "[image]".to_string()
                    } else {
                        format!("[image: {alt}]")
                    });
                }
            }
        }
    }

    let mut lines = Vec::new();
    blocks_text(blocks, &mut lines);
    lines.join("\n\n")
}

/// Parse an HTML part into blocks. Returns whatever survived: an empty vec
/// when the markup was all chrome, trackers, and hidden preheaders, in
/// which case the caller falls back to the plain paragraphs.
pub fn parse_html_body(html: &str) -> RichBody {
    let dom = match tl::parse(html, ParserOptions::default()) {
        Ok(dom) => dom,
        Err(_) => return Vec::new(),
    };
    let parser = dom.parser();
    // Start at <body> when there is one so <head> metadata never leaks in;
    // otherwise walk the whole document.
    let roots: Vec<NodeHandle> = dom
        .query_selector("body")
        .and_then(|mut found| found.next())
        .and_then(|handle| handle.get(parser))
        .and_then(|node| node.as_tag())
        .map(|body| body.children().top().to_vec())
        .unwrap_or_else(|| dom.children().to_vec());

    let mut walker = Walker {
        parser,
        out: Vec::new(),
        para: Vec::new(),
    };
    let ctx = InlineCtx::default();
    walker.walk_all(&roots, &ctx);
    walker.flush_para();
    walker.out
}

/// Inline context accumulated down the tree: emphasis flags, the enclosing
/// anchor, and whether whitespace must survive verbatim (`<pre>`).
#[derive(Clone, Default)]
struct InlineCtx {
    bold: bool,
    italic: bool,
    link: Option<String>,
    verbatim: bool,
}

struct Walker<'a> {
    parser: &'a Parser<'a>,
    out: Vec<RichBlock>,
    para: Vec<RichSpan>,
}

impl<'a> Walker<'a> {
    fn push_block(&mut self, block: RichBlock) {
        if self.out.len() < MAX_BLOCKS {
            self.out.push(block);
        }
    }

    /// Close the open paragraph, if it holds anything but air.
    fn flush_para(&mut self) {
        let mut spans = std::mem::take(&mut self.para);
        trim_span_edges(&mut spans);
        if spans.iter().any(|span| !span.text.trim().is_empty()) {
            self.push_block(RichBlock::Paragraph(spans));
        }
    }

    /// Append text to the open paragraph, collapsing whitespace runs unless
    /// verbatim. A span merges into its predecessor when the flags match,
    /// so `<b>a</b><b>b</b>` stays one run instead of two adjacent divs.
    fn push_text(&mut self, text: &str, ctx: &InlineCtx) {
        let collapsed = if ctx.verbatim {
            text.to_string()
        } else {
            collapse_ws(text)
        };
        if collapsed.is_empty() {
            return;
        }
        let mut collapsed = collapsed;
        // Runs join without doubling the space they share: "foo " plus
        // " bar" reads as one gap, not two.
        if let Some(last) = self.para.last()
            && last.bold == ctx.bold
            && last.italic == ctx.italic
            && last.link == ctx.link
            && last.text.ends_with([' ', '\n'])
            && let Some(stripped) = collapsed.strip_prefix([' ', '\n'])
        {
            collapsed = stripped.to_string();
        }
        if collapsed.is_empty() {
            return;
        }
        if let Some(last) = self.para.last_mut()
            && last.bold == ctx.bold
            && last.italic == ctx.italic
            && last.link == ctx.link
        {
            last.text.push_str(&collapsed);
        } else {
            self.para.push(RichSpan {
                text: collapsed,
                bold: ctx.bold,
                italic: ctx.italic,
                link: ctx.link.clone(),
            });
        }
    }

    fn walk_all(&mut self, handles: &[NodeHandle], ctx: &InlineCtx) {
        for handle in handles {
            self.walk_node(handle, ctx);
        }
    }

    fn walk_node(&mut self, handle: &NodeHandle, ctx: &InlineCtx) {
        let Some(node) = handle.get(self.parser) else {
            return;
        };
        match node {
            Node::Raw(bytes) => {
                self.push_text(&decode_entities(&bytes.as_utf8_str()), ctx);
            }
            Node::Comment(_) => {}
            Node::Tag(tag) => self.walk_tag(tag, ctx),
        }
    }

    fn walk_tag(&mut self, tag: &tl::HTMLTag<'a>, ctx: &InlineCtx) {
        let name = tag.name().as_utf8_str().to_ascii_lowercase();
        if is_hidden(tag) {
            return;
        }
        match name.as_str() {
            // Never content.
            "script" | "style" | "head" | "title" | "meta" | "link" | "base" | "template"
            | "iframe" | "object" | "embed" | "video" | "audio" | "canvas" | "svg" | "input"
            | "select" | "textarea" | "noscript" => {}
            "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "center"
            | "figure" | "figcaption" | "pre" | "table" | "tr" => {
                self.flush_para();
                let mut ctx = ctx.clone();
                if name.as_str() == "pre" {
                    ctx.verbatim = true;
                }
                let children = tag.children().top().to_vec();
                self.walk_all(&children, &ctx);
                self.flush_para();
            }
            "td" | "th" => {
                // Cells stack: tables are layout here, and a cell boundary
                // is where one thought ends and the next begins.
                self.flush_para();
                let children = tag.children().top().to_vec();
                self.walk_all(&children, ctx);
                self.flush_para();
            }
            "br" => self.push_break(),
            "hr" => {
                self.flush_para();
                self.push_block(RichBlock::Rule);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush_para();
                let level = (name.as_bytes()[1] - b'0').min(3);
                let mut heading = HeadingWalker {
                    parser: self.parser,
                    spans: Vec::new(),
                };
                let children = tag.children().top().to_vec();
                heading.walk_all(&children, ctx);
                trim_span_edges(&mut heading.spans);
                if heading
                    .spans
                    .iter()
                    .any(|span| !span.text.trim().is_empty())
                {
                    self.push_block(RichBlock::Heading {
                        level,
                        spans: heading.spans,
                    });
                }
            }
            "ul" | "ol" => self.walk_list(tag, ctx, name.as_str() == "ol"),
            // A list item outside any list is a paragraph that lost its
            // marker, not a reason to drop the text.
            "li" => {
                self.flush_para();
                let children = tag.children().top().to_vec();
                self.walk_all(&children, ctx);
                self.flush_para();
            }
            "blockquote" => {
                self.flush_para();
                let children = tag.children().top().to_vec();
                let mut quoted = Walker {
                    parser: self.parser,
                    out: Vec::new(),
                    para: Vec::new(),
                };
                quoted.walk_all(&children, ctx);
                quoted.flush_para();
                if !quoted.out.is_empty() {
                    self.push_block(RichBlock::Quote(quoted.out));
                }
            }
            "a" => {
                let mut ctx = ctx.clone();
                ctx.link = anchor_target(tag);
                let children = tag.children().top().to_vec();
                self.walk_all(&children, &ctx);
            }
            "b" | "strong" => {
                let mut ctx = ctx.clone();
                ctx.bold = true;
                let children = tag.children().top().to_vec();
                self.walk_all(&children, &ctx);
            }
            "i" | "em" => {
                let mut ctx = ctx.clone();
                ctx.italic = true;
                let children = tag.children().top().to_vec();
                self.walk_all(&children, &ctx);
            }
            "img" => {
                self.flush_para();
                if let Some(image) = parse_image(tag, &ctx.link) {
                    self.push_block(RichBlock::Image {
                        src: image.0,
                        alt: image.1,
                        link: image.2,
                    });
                }
            }
            _ => {
                let children = tag.children().top().to_vec();
                self.walk_all(&children, ctx);
            }
        }
    }

    /// A line break joins the current line rather than ending the paragraph:
    /// `<br><br>` pairs still separate thoughts, because the second break
    /// lands on an empty line the paragraph splitter below will see.
    fn push_break(&mut self) {
        if let Some(last) = self.para.last_mut() {
            last.text.push('\n');
        } else {
            self.para.push(RichSpan {
                text: "\n".to_string(),
                bold: false,
                italic: false,
                link: None,
            });
        }
    }

    fn walk_list(&mut self, tag: &tl::HTMLTag<'a>, ctx: &InlineCtx, ordered: bool) {
        self.flush_para();
        let mut collector = ListCollector {
            parser: self.parser,
            items: Vec::new(),
        };
        collector.collect(tag, ctx);
        collector
            .items
            .retain(|item| item.iter().any(|span| !span.text.trim().is_empty()));
        if !collector.items.is_empty() {
            self.push_block(RichBlock::List {
                ordered,
                items: collector.items,
            });
        }
    }
}

/// Gathers one list item's spans, detouring nested lists into siblings.
struct ItemWalker<'a> {
    parser: &'a Parser<'a>,
    item: Vec<RichSpan>,
    items: Vec<Vec<RichSpan>>,
}

impl<'a> ItemWalker<'a> {
    fn flush_item(&mut self) {
        let mut spans = std::mem::take(&mut self.item);
        trim_span_edges(&mut spans);
        if spans.iter().any(|span| !span.text.trim().is_empty()) {
            self.items.push(spans);
        }
    }

    fn walk_all(&mut self, handles: &[NodeHandle], ctx: &InlineCtx) {
        // Reuse the block walker for inline content, but intercept lists:
        // rather than emitting a List block, a nested list closes this item
        // and contributes its own items as siblings.
        for handle in handles {
            let Some(node) = handle.get(self.parser) else {
                continue;
            };
            match node {
                Node::Raw(bytes) => {
                    let text = collapse_ws(&decode_entities(&bytes.as_utf8_str()));
                    if !text.is_empty() {
                        self.push_span(text, ctx);
                    }
                }
                Node::Comment(_) => {}
                Node::Tag(tag) => {
                    let name = tag.name().as_utf8_str().to_ascii_lowercase();
                    if is_hidden(tag) {
                        continue;
                    }
                    match name.as_str() {
                        "ul" | "ol" => {
                            self.flush_item();
                            let mut nested = ListCollector {
                                parser: self.parser,
                                items: Vec::new(),
                            };
                            nested.collect(tag, ctx);
                            self.items.extend(nested.items);
                        }
                        "br" => self.push_span("\n".to_string(), ctx),
                        "a" => {
                            let mut ctx = ctx.clone();
                            ctx.link = anchor_target(tag);
                            let children = tag.children().top().to_vec();
                            self.walk_all(&children, &ctx);
                        }
                        "b" | "strong" => {
                            let mut ctx = ctx.clone();
                            ctx.bold = true;
                            let children = tag.children().top().to_vec();
                            self.walk_all(&children, &ctx);
                        }
                        "i" | "em" => {
                            let mut ctx = ctx.clone();
                            ctx.italic = true;
                            let children = tag.children().top().to_vec();
                            self.walk_all(&children, &ctx);
                        }
                        "img" => {
                            // Images have no inline form: the alt text stands
                            // in, and the picture itself waits for the block
                            // pass, which never comes inside a list item.
                            if let Some(alt) = image_alt(tag)
                                && !alt.trim().is_empty()
                            {
                                self.push_span(format!("[image: {alt}]"), ctx);
                            }
                        }
                        _ => {
                            let children = tag.children().top().to_vec();
                            self.walk_all(&children, ctx);
                        }
                    }
                }
            }
        }
    }

    fn push_span(&mut self, text: String, ctx: &InlineCtx) {
        if let Some(last) = self.item.last_mut()
            && last.bold == ctx.bold
            && last.italic == ctx.italic
            && last.link == ctx.link
        {
            last.text.push_str(&text);
        } else {
            self.item.push(RichSpan {
                text,
                bold: ctx.bold,
                italic: ctx.italic,
                link: ctx.link.clone(),
            });
        }
    }
}

/// Collects a nested list's items for flattening into its parent.
struct ListCollector<'a> {
    parser: &'a Parser<'a>,
    items: Vec<Vec<RichSpan>>,
}

impl<'a> ListCollector<'a> {
    fn collect(&mut self, tag: &tl::HTMLTag<'a>, ctx: &InlineCtx) {
        let children = tag.children().top().to_vec();
        for handle in &children {
            let Some(node) = handle.get(self.parser) else {
                continue;
            };
            let Node::Tag(child) = node else { continue };
            if child.name().as_utf8_str().to_ascii_lowercase() != "li" {
                continue;
            }
            // A nested list inside an item flattens into siblings: two
            // levels of bullets for a sale flyer is chrome, not structure.
            let mut item_walker = ItemWalker {
                parser: self.parser,
                item: Vec::new(),
                items: Vec::new(),
            };
            let item_children = child.children().top().to_vec();
            item_walker.walk_all(&item_children, ctx);
            item_walker.flush_item();
            self.items.extend(item_walker.items);
        }
    }
}

/// Collects heading spans with the same inline rules as body text.
struct HeadingWalker<'a> {
    parser: &'a Parser<'a>,
    spans: Vec<RichSpan>,
}

impl<'a> HeadingWalker<'a> {
    fn walk_all(&mut self, handles: &[NodeHandle], ctx: &InlineCtx) {
        for handle in handles {
            let Some(node) = handle.get(self.parser) else {
                continue;
            };
            match node {
                Node::Raw(bytes) => {
                    let text = collapse_ws(&decode_entities(&bytes.as_utf8_str()));
                    if !text.is_empty() {
                        self.push_span(text, ctx);
                    }
                }
                Node::Comment(_) => {}
                Node::Tag(tag) => {
                    if is_hidden(tag) {
                        continue;
                    }
                    let name = tag.name().as_utf8_str().to_ascii_lowercase();
                    match name.as_str() {
                        "br" => self.push_span("\n".to_string(), ctx),
                        "a" => {
                            let mut ctx = ctx.clone();
                            ctx.link = anchor_target(tag);
                            let children = tag.children().top().to_vec();
                            self.walk_all(&children, &ctx);
                        }
                        "b" | "strong" => {
                            let mut ctx = ctx.clone();
                            ctx.bold = true;
                            let children = tag.children().top().to_vec();
                            self.walk_all(&children, &ctx);
                        }
                        "i" | "em" => {
                            let mut ctx = ctx.clone();
                            ctx.italic = true;
                            let children = tag.children().top().to_vec();
                            self.walk_all(&children, &ctx);
                        }
                        _ => {
                            let children = tag.children().top().to_vec();
                            self.walk_all(&children, ctx);
                        }
                    }
                }
            }
        }
    }

    fn push_span(&mut self, text: String, ctx: &InlineCtx) {
        if let Some(last) = self.spans.last_mut()
            && last.bold == ctx.bold
            && last.italic == ctx.italic
            && last.link == ctx.link
        {
            last.text.push_str(&text);
        } else {
            self.spans.push(RichSpan {
                text,
                bold: ctx.bold,
                italic: ctx.italic,
                link: ctx.link.clone(),
            });
        }
    }
}

/// The one deliberate style sniff: `display:none` hides preheaders and
/// spacer hacks, and honouring that single declaration is what keeps them
/// out of the reading view. Everything else in `style` is ignored — there
/// is no CSS engine here, on purpose.
fn is_hidden(tag: &tl::HTMLTag<'_>) -> bool {
    if tag.attributes().get("hidden").flatten().is_some() {
        return true;
    }
    let Some(style) = tag.attributes().get("style").flatten() else {
        return false;
    };
    style
        .as_utf8_str()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase()
        .contains("display:none")
}

/// An anchor's target, when it is a page we may open: absolute `https`
/// only. Anything else (`mailto:`, fragments, relative paths) keeps its
/// text and drops the click.
fn anchor_target(tag: &tl::HTMLTag<'_>) -> Option<String> {
    let href = tag
        .attributes()
        .get("href")
        .flatten()
        .map(|bytes| bytes.as_utf8_str().trim().to_string())?;
    https_url(&href)
}

/// Keep absolute `https` URLs; everything else cannot be fetched or opened
/// safely without a base page, so it is dropped at the boundary.
fn https_url(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") && url.len() > "https://".len() {
        Some(url.to_string())
    } else {
        None
    }
}

/// An image's `(src, alt, link)`: `None` when there is nothing to show —
/// a non-`https` source, or a 1×1 tracking pixel, which is telemetry, not
/// content.
fn parse_image(
    tag: &tl::HTMLTag<'_>,
    link: &Option<String>,
) -> Option<(String, String, Option<String>)> {
    let src = tag
        .attributes()
        .get("src")
        .flatten()
        .map(|bytes| bytes.as_utf8_str().trim().to_string())?;
    let src = https_url(&src)?;
    if is_tracker(tag) {
        return None;
    }
    let alt = tag
        .attributes()
        .get("alt")
        .flatten()
        .map(|bytes| decode_entities(&bytes.as_utf8_str()))
        .unwrap_or_default();
    Some((src, alt.trim().to_string(), link.clone()))
}

/// Alt text for inline image fallbacks, without fetching anything.
fn image_alt(tag: &tl::HTMLTag<'_>) -> Option<String> {
    tag.attributes()
        .get("alt")
        .flatten()
        .map(|bytes| decode_entities(&bytes.as_utf8_str()).trim().to_string())
}

/// A 1×1 pixel image is an open-tracker, not a picture. Spacers share the
/// dimensions, and both are invisible either way, so both go.
fn is_tracker(tag: &tl::HTMLTag<'_>) -> bool {
    fn dimension(tag: &tl::HTMLTag<'_>, name: &str) -> Option<u32> {
        tag.attributes().get(name).flatten().and_then(|bytes| {
            bytes
                .as_utf8_str()
                .trim()
                .trim_end_matches("px")
                .trim()
                .parse()
                .ok()
        })
    }
    matches!(
        (dimension(tag, "width"), dimension(tag, "height")),
        (Some(1), Some(1))
    )
}

/// Collapse whitespace runs to a single space. Newlines from markup
/// indentation are spacing, not content; `<br>` and `<pre>` bypass this.
/// One leading and one trailing space survive, because they are the joints
/// between this run and its neighbours: without them `Write <b>bob</b>`
/// would glue into `Writebob`.
fn collapse_ws(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space {
                out.push(' ');
            }
            pending_space = false;
            out.push(c);
        }
    }
    if pending_space {
        out.push(' ');
    }
    out
}

/// Trim the paragraph's outer edges: the joints between runs keep interior
/// spacing, but a paragraph starts and ends on content, not air. Runs left
/// empty by the trim are dropped.
fn trim_span_edges(spans: &mut Vec<RichSpan>) {
    if let Some(first) = spans.first_mut() {
        let trimmed = first.text.trim_start().to_string();
        first.text = trimmed;
    }
    if let Some(last) = spans.last_mut() {
        let trimmed = last.text.trim_end().to_string();
        last.text = trimmed;
    }
    while spans.first().is_some_and(|span| span.text.is_empty()) {
        spans.remove(0);
    }
    while spans.last().is_some_and(|span| span.text.is_empty()) {
        spans.pop();
    }
}

/// The entities ESP markup actually emits. Single pass, matching the plain
/// path's decoder so both pipelines read the same.
fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shop_mail() -> &'static str {
        concat!(
            "<html><head><title>Sale</title><style>.x{color:red}</style></head>",
            "<body>",
            "<div style=\"display:none\">Preheader junk you should never see</div>",
            "<!--[if mso]><table><tr><td>Outlook only</td></tr></table><![endif]-->",
            "<h1>Tokyo Game Show <a href=\"https://shop.example/e\">Sale</a></h1>",
            "<p>Save on <b>1,000+ games</b> from <i>Square Enix</i> &amp; more.</p>",
            "<a href=\"https://shop.example/hero\"><img src=\"https://img.example/hero.jpg\" alt=\"Hero art\"></a>",
            "<ul><li><a href=\"https://shop.example/sifu\">Sifu</a> — UP TO -78% OFF</li>",
            "<li>Plain item</li></ul>",
            "<ol><li>First</li><li>Second</li></ol>",
            "<blockquote><p>Quoted <b>praise</b></p></blockquote>",
            "<hr>",
            "<table><tr><td>Cell one</td><td>Cell two</td></tr></table>",
            "<img src=\"https://img.example/open.gif\" width=\"1\" height=\"1\">",
            "<img src=\"http://img.example/plain.jpg\" alt=\"Nope\">",
            "<p>Write <a href=\"mailto:bob@example.com\">bob</a> any time.</p>",
            "<script>evil();</script>",
            "</body></html>",
        )
    }

    #[test]
    fn head_style_and_hidden_preheader_never_render() {
        let blocks = parse_html_body(shop_mail());
        let text = plain_text(&blocks);
        assert!(
            !text.contains("Preheader junk"),
            "display:none must hide: {text}"
        );
        assert!(
            !text.contains("Outlook only"),
            "MSO conditionals are comments: {text}"
        );
        assert!(
            !text.contains("color:red"),
            "style content must not leak: {text}"
        );
        assert!(
            !text.contains("evil();"),
            "script content must not leak: {text}"
        );
        assert!(
            !text.contains("Sale</title>") && !text.contains("<title>"),
            "head must not leak"
        );
    }

    #[test]
    fn headings_links_and_emphasis_parse() {
        let blocks = parse_html_body(shop_mail());
        let RichBlock::Heading { level, spans } = &blocks[0] else {
            panic!("first block must be the heading, got {:?}", blocks.first());
        };
        assert_eq!(*level, 1);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].link.as_deref(), Some("https://shop.example/e"));

        let RichBlock::Paragraph(spans) = &blocks[1] else {
            panic!("second block must be a paragraph, got {:?}", blocks.get(1));
        };
        assert!(
            spans
                .iter()
                .any(|span| span.bold && span.text.contains("1,000+"))
        );
        assert!(
            spans
                .iter()
                .any(|span| span.italic && span.text.contains("Square Enix"))
        );
        assert!(
            spans
                .iter()
                .any(|span| span.text.contains('&') && !span.text.contains("amp;"))
        );
    }

    #[test]
    fn linked_images_become_clickable_figures() {
        let blocks = parse_html_body(shop_mail());
        let image = blocks.iter().find_map(|block| match block {
            RichBlock::Image { src, alt, link } => Some((src, alt, link)),
            _ => None,
        });
        let Some((src, alt, link)) = image else {
            panic!("the hero image must survive as a figure: {blocks:?}");
        };
        assert_eq!(src, "https://img.example/hero.jpg");
        assert_eq!(alt, "Hero art");
        assert_eq!(link.as_deref(), Some("https://shop.example/hero"));
    }

    #[test]
    fn trackers_and_plain_http_images_are_dropped() {
        let blocks = parse_html_body(shop_mail());
        let text = plain_text(&blocks);
        assert!(
            !text.contains("open.gif"),
            "1x1 pixels are telemetry: {text}"
        );
        assert!(
            !text.contains("plain.jpg"),
            "http images never load: {text}"
        );
        assert!(
            !blocks.iter().any(|block| matches!(
                block,
                RichBlock::Image { src, .. } if src.contains("open.gif") || src.contains("plain.jpg")
            )),
            "neither may survive as a block: {blocks:?}"
        );
    }

    #[test]
    fn lists_tables_quotes_and_rules_flatten() {
        let blocks = parse_html_body(shop_mail());
        let lists: Vec<_> = blocks
            .iter()
            .filter_map(|block| match block {
                RichBlock::List { ordered, items } => Some((ordered, items.len())),
                _ => None,
            })
            .collect();
        assert_eq!(
            lists,
            vec![(&false, 2), (&true, 2)],
            "ul then ol: {blocks:?}"
        );

        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, RichBlock::Quote(_))),
            "the quote must survive: {blocks:?}"
        );
        assert!(
            blocks.iter().any(|block| matches!(block, RichBlock::Rule)),
            "the rule must survive: {blocks:?}"
        );
        let text = plain_text(&blocks);
        assert!(
            text.contains("Cell one") && text.contains("Cell two"),
            "table cells stack as text: {text}"
        );
    }

    #[test]
    fn mailto_keeps_text_and_drops_the_click() {
        let blocks = parse_html_body(shop_mail());
        let text = plain_text(&blocks);
        assert!(
            text.contains("bob"),
            "the addressee name must survive: {text}"
        );
        let linked = blocks.iter().any(|block| match block {
            RichBlock::Paragraph(spans) => spans
                .iter()
                .any(|span| span.link.as_deref() == Some("mailto:bob@example.com")),
            _ => false,
        });
        assert!(!linked, "mailto: must never become a click target");
    }

    #[test]
    fn garbage_in_empty_vec_out() {
        assert!(parse_html_body("").is_empty());
        assert!(parse_html_body("<div><span>").is_empty());
        assert!(parse_html_body("<script>only();</script>").is_empty());
    }

    #[test]
    fn plain_paragraphs_round_trip_through_blocks() {
        let blocks = text_blocks(vec!["Hello".to_string(), "World".to_string()]);
        assert_eq!(plain_text(&blocks), "Hello\n\nWorld");
    }

    #[test]
    fn image_sources_collect_in_order_without_duplicates() {
        let blocks = vec![
            RichBlock::Paragraph(vec![RichSpan::plain("Hi")]),
            RichBlock::Image {
                src: "https://img.example/a.jpg".to_string(),
                alt: String::new(),
                link: None,
            },
            RichBlock::Quote(vec![RichBlock::Image {
                src: "https://img.example/a.jpg".to_string(),
                alt: String::new(),
                link: None,
            }]),
            RichBlock::Image {
                src: "https://img.example/b.jpg".to_string(),
                alt: String::new(),
                link: None,
            },
        ];
        assert_eq!(
            image_sources(&blocks),
            vec![
                "https://img.example/a.jpg".to_string(),
                "https://img.example/b.jpg".to_string(),
            ]
        );
    }

    #[test]
    fn links_stay_visible_in_plain_projection() {
        let blocks = vec![RichBlock::Paragraph(vec![
            RichSpan::plain("Shop "),
            RichSpan {
                text: "sale".to_string(),
                bold: false,
                italic: false,
                link: Some("https://shop.example/e".to_string()),
            },
        ])];
        assert_eq!(plain_text(&blocks), "Shop sale <https://shop.example/e>");
    }
}
