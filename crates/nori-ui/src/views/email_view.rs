use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, FocusHandle, Image, ImageSource, IntoElement, RenderOnce, Window, div, img, prelude::*, px,
};

use super::super::components::{Button, ButtonStyle, Icon};
use crate::model::Email;
use crate::theme::Theme;
use nori_gmail::{RichBlock, RichSpan};

type EmailActionHandler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// One remote picture's load state. Cloned into the reading view on every
/// render, so it is an `Arc` behind a tag rather than bytes on the move.
#[derive(Clone)]
pub enum ImageSlot {
    Loading,
    Loaded(Arc<Image>),
    Failed,
}

#[derive(IntoElement)]
pub struct EmailView {
    email: Email,
    focus_handle: FocusHandle,
    /// Remote images by source URL. Missing entries are still fetching (or
    /// were never asked for) and render as their alt text.
    images: HashMap<String, ImageSlot>,
    on_reply: EmailActionHandler,
    on_reply_all: EmailActionHandler,
    on_forward: EmailActionHandler,
    on_toggle_pin: EmailActionHandler,
}

impl EmailView {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        email: Email,
        focus_handle: FocusHandle,
        images: HashMap<String, ImageSlot>,
        on_reply: impl Fn(&mut Window, &mut App) + 'static,
        on_reply_all: impl Fn(&mut Window, &mut App) + 'static,
        on_forward: impl Fn(&mut Window, &mut App) + 'static,
        on_toggle_pin: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            email,
            focus_handle,
            images,
            on_reply: Rc::new(on_reply),
            on_reply_all: Rc::new(on_reply_all),
            on_forward: Rc::new(on_forward),
            on_toggle_pin: Rc::new(on_toggle_pin),
        }
    }
}

impl RenderOnce for EmailView {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::current(cx);
        let email = self.email;
        let images = self.images;
        let on_reply = self.on_reply.clone();
        let on_reply_all = self.on_reply_all.clone();
        let on_forward = self.on_forward.clone();
        let on_toggle_pin = self.on_toggle_pin.clone();
        let pinned = email.pinned;

        div()
            .id(format!("email-view-{}", email.id))
            .track_focus(&self.focus_handle)
            .tab_index(0)
            .focus_visible(|style| style.border_color(theme.focus))
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_scroll()
            .bg(theme.canvas)
            .child(
                div()
                    .w_full()
                    .max_w(px(720.))
                    .px(px(44.))
                    .pt(px(32.))
                    .pb(px(48.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(email.subject.clone()),
                    )
                    .child(
                        div()
                            .mt(px(17.))
                            .flex()
                            .items_start()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(theme.text)
                                            .child(format!("{} <{}>", email.sender, email.address)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.muted)
                                            .child(format!("To: {}", email.recipients.join(", "))),
                                    ),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.ghost)
                                    .child(email.full_date.clone()),
                            ),
                    )
                    .child(div().mt(px(23.)).h(px(1.)).bg(theme.hairline))
                    .child(div().mt(px(25.)).flex().flex_col().gap(px(15.)).children(
                        email.body.iter().enumerate().map(|(index, block)| {
                            let mut counter = 0usize;
                            render_body_block(index, block, &images, theme, &mut counter)
                        }),
                    ))
                    .child(div().mt(px(28.)).h(px(1.)).bg(theme.hairline))
                    .child(
                        div()
                            .mt(px(15.))
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .child(
                                Button::new("reply-button", "Reply")
                                    .dense()
                                    .scaled(1.05)
                                    .icon(Icon::new("icons/reply.svg", 13., theme.muted))
                                    .style(ButtonStyle::Subtle)
                                    .on_click(move |_event, window, cx| on_reply(window, cx)),
                            )
                            .child(
                                Button::new("reply-all-button", "Reply All")
                                    .dense()
                                    .scaled(1.05)
                                    .icon(Icon::new("icons/reply-all.svg", 13., theme.muted))
                                    .style(ButtonStyle::Subtle)
                                    .on_click(move |_event, window, cx| on_reply_all(window, cx)),
                            )
                            .child(
                                Button::new("forward-button", "Forward")
                                    .dense()
                                    .scaled(1.05)
                                    .icon(Icon::new("icons/forward.svg", 13., theme.muted))
                                    .style(ButtonStyle::Subtle)
                                    .on_click(move |_event, window, cx| on_forward(window, cx)),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id(if pinned { "email-unpin" } else { "email-pin" })
                                    .debug_selector(move || {
                                        if pinned {
                                            "email-unpin".to_string()
                                        } else {
                                            "email-pin".to_string()
                                        }
                                    })
                                    .child(
                                        Button::new(
                                            "email-pin-button",
                                            if pinned { "Unpin" } else { "Pin" },
                                        )
                                        .dense()
                                        .scaled(1.05)
                                        .icon(Icon::new(
                                            if pinned {
                                                "icons/pin.svg"
                                            } else {
                                                "icons/pin-off.svg"
                                            },
                                            13.,
                                            if pinned { theme.accent } else { theme.muted },
                                        ))
                                        .style(ButtonStyle::Subtle)
                                        .on_click(
                                            move |_event, window, cx| on_toggle_pin(window, cx),
                                        ),
                                    ),
                            ),
                    ),
            )
    }
}

/// One inline piece of a body paragraph: plain text, or a link.
#[derive(Debug, PartialEq, Eq)]
enum BodySegment<'a> {
    Text(&'a str),
    Link(&'a str),
}

/// Split a paragraph into text and link runs. Both the bare form the sync
/// stage normalises to and the angle-wrapped form it may never have seen
/// (sample mail bypasses sync) become exactly one link each: a wrapped URL
/// is never linkified twice, brackets and all.
fn split_links(paragraph: &str) -> Vec<BodySegment<'_>> {
    let mut segments = Vec::new();
    // Start of the pending text run, as a byte offset. Text accumulates
    // untouched until a real link starts, so a bare scheme with nothing
    // after it never splits the run it sits in.
    let mut text_start = 0usize;
    let mut cursor = 0usize;
    while cursor < paragraph.len() {
        let Some(relative) = next_scheme(&paragraph[cursor..]) else {
            break;
        };
        let start = cursor + relative;
        let scheme_len = if paragraph[start..].starts_with("https://") {
            8
        } else {
            7
        };
        let end = start + url_end(&paragraph[start..]);
        if end <= start + scheme_len {
            cursor = end;
            continue;
        }
        let url = &paragraph[start..end];
        // An angle-wrapped URL donates its brackets: the `<` before the
        // scheme and the `>` after the URL are consumed with the link.
        let wrapped = paragraph[..start].ends_with('<') && paragraph[end..].starts_with('>');
        let text_end = if wrapped { start - 1 } else { start };
        if text_end > text_start {
            segments.push(BodySegment::Text(&paragraph[text_start..text_end]));
        }
        segments.push(BodySegment::Link(url));
        cursor = end + if wrapped { 1 } else { 0 };
        text_start = cursor;
    }
    if text_start < paragraph.len() {
        segments.push(BodySegment::Text(&paragraph[text_start..]));
    }
    segments
}

/// Byte offset of the next `http://` or `https://`, if any.
fn next_scheme(text: &str) -> Option<usize> {
    text.find("http://")
        .into_iter()
        .chain(text.find("https://"))
        .min()
}

/// Byte length of the URL at the start of `text`, which must begin with a
/// scheme. Runs to whitespace, a quote, or an angle bracket, then sheds
/// trailing punctuation that prose glues on: commas, periods, sentence
/// ends. Closing parens shed only while they outnumber opening ones, so
/// prose like `(see https://…/X_(thing))` keeps the link's own pair and
/// drops the sentence's.
fn url_end(text: &str) -> usize {
    let mut end = text.len();
    for (i, c) in text.char_indices() {
        if c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '`') {
            end = i;
            break;
        }
    }
    let mut url = &text[..end];
    while url.ends_with(['.', ',', ';', ':', '!', '?']) {
        url = &url[..url.len() - 1];
    }
    let opens = url.bytes().filter(|&byte| byte == b'(').count();
    let mut closes = url.bytes().filter(|&byte| byte == b')').count();
    while url.ends_with(')') && closes > opens {
        url = &url[..url.len() - 1];
        closes -= 1;
    }
    url.len()
}

/// One body block. `index` is the block's position in the body and scopes
/// the debug selectors; `counter` numbers the click targets inside it so
/// every link and image on the page stays addressable.
fn render_body_block(
    index: usize,
    block: &RichBlock,
    images: &HashMap<String, ImageSlot>,
    theme: Theme,
    counter: &mut usize,
) -> gpui::AnyElement {
    match block {
        RichBlock::Paragraph(spans) => render_spans(index, spans, false, theme, counter),
        RichBlock::Heading { level, spans } => {
            let size = match level {
                1 => px(20.),
                2 => px(17.),
                _ => px(15.),
            };
            div()
                .mt(px(6.))
                .text_size(size)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(render_spans(index, spans, true, theme, counter))
                .into_any_element()
        }
        RichBlock::List { ordered, items } => div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .children(items.iter().enumerate().map(|(item, spans)| {
                let marker = if *ordered {
                    format!("{}.", item + 1)
                } else {
                    "•".to_string()
                };
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_none()
                            .w(px(20.))
                            .text_align(gpui::TextAlign::Right)
                            .text_size(px(14.))
                            .text_color(theme.ghost)
                            .child(marker),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(render_spans(index, spans, false, theme, counter)),
                    )
                    .into_any_element()
            }))
            .into_any_element(),
        RichBlock::Quote(blocks) => div()
            .border_l_2()
            .border_color(theme.strong_border)
            .pl(px(12.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .children(
                blocks
                    .iter()
                    .map(|block| render_body_block(index, block, images, theme, counter)),
            )
            .into_any_element(),
        RichBlock::Rule => div()
            .my(px(4.))
            .h(px(1.))
            .bg(theme.hairline)
            .into_any_element(),
        RichBlock::Image { src, alt, link } => {
            render_body_image(index, src, alt, link.as_deref(), images, theme)
        }
    }
}

/// One figure: the picture when it has arrived, its alt text while loading
/// or when loading failed. Wrapped in its anchor when the image itself is
/// the click target, the way product cards link to the store.
fn render_body_image(
    index: usize,
    src: &str,
    alt: &str,
    link: Option<&str>,
    images: &HashMap<String, ImageSlot>,
    theme: Theme,
) -> gpui::AnyElement {
    let selector = format!("email-body-img-{index}");
    let picture: gpui::AnyElement = match images.get(src) {
        Some(ImageSlot::Loaded(image)) => img(ImageSource::Image(image.clone()))
            .w_full()
            .into_any_element(),
        _ => {
            let caption = if alt.is_empty() {
                "Loading image…".to_string()
            } else {
                alt.to_string()
            };
            div()
                .w_full()
                .py(px(24.))
                .flex()
                .justify_center()
                .border_1()
                .border_color(theme.hairline)
                .text_size(px(12.))
                .text_color(theme.ghost)
                .child(caption)
                .into_any_element()
        }
    };
    let framed = div()
        .id(selector.clone())
        .debug_selector(move || selector.clone())
        .w_full()
        .child(picture)
        .into_any_element();
    match link {
        Some(url) => {
            let opener = url.to_string();
            let link_id = format!("email-body-img-link-{index}");
            div()
                .id(link_id.clone())
                .debug_selector(move || link_id.clone())
                .cursor_pointer()
                .aria_label(format!("Open link {opener}"))
                .on_click(move |_, _, cx| {
                    cx.open_url(&opener);
                })
                .child(framed)
                .into_any_element()
        }
        None => framed,
    }
}

/// One run of spans — a paragraph, a heading line, a list item. Plain spans
/// are linkified on the way through, so bare URLs in HTML text nodes click
/// exactly like the ones Phase 1 found in plain bodies; spans the parser
/// already linked render as-is.
fn render_spans(
    index: usize,
    spans: &[RichSpan],
    force_bold: bool,
    theme: Theme,
    counter: &mut usize,
) -> gpui::AnyElement {
    // The common case stays one plain div: yesterday's rendering, untouched.
    if let [span] = spans
        && !span.bold
        && !span.italic
        && span.link.is_none()
        && !force_bold
        && split_links(&span.text)
            .iter()
            .all(|segment| matches!(segment, BodySegment::Text(_)))
    {
        return div()
            .text_size(px(14.))
            .line_height(px(22.))
            .text_color(theme.text)
            .child(span.text.clone())
            .into_any_element();
    }
    let mut children = Vec::new();
    for span in spans {
        let runs = match &span.link {
            // The parser's link is authoritative: its text stays whole.
            Some(_) => vec![SpanRun::linked(&span.text, span)],
            None => split_links(&span.text)
                .into_iter()
                .map(|segment| match segment {
                    BodySegment::Text(text) => SpanRun::text(text, span),
                    BodySegment::Link(url) => SpanRun::linked(url, span),
                })
                .collect(),
        };
        for run in runs {
            children.push(render_span_run(index, run, force_bold, theme, counter));
        }
    }
    div()
        .flex()
        .flex_wrap()
        .text_size(px(14.))
        .line_height(px(22.))
        .children(children)
        .into_any_element()
}

/// One styled run inside a wrapping row.
struct SpanRun<'a> {
    text: &'a str,
    bold: bool,
    italic: bool,
    link: Option<&'a str>,
}

impl<'a> SpanRun<'a> {
    fn text(text: &'a str, span: &'a RichSpan) -> Self {
        Self {
            text,
            bold: span.bold,
            italic: span.italic,
            link: None,
        }
    }

    fn linked(url: &'a str, span: &'a RichSpan) -> Self {
        // A parser link keeps its own display text; a bare URL is its own
        // text, which the caller already arranged.
        let text = match &span.link {
            Some(_) => span.text.as_str(),
            None => url,
        };
        Self {
            text,
            bold: span.bold,
            italic: span.italic,
            link: Some(url),
        }
    }
}

fn render_span_run(
    index: usize,
    run: SpanRun<'_>,
    force_bold: bool,
    theme: Theme,
    counter: &mut usize,
) -> gpui::AnyElement {
    let weight = if run.bold || force_bold {
        gpui::FontWeight::SEMIBOLD
    } else {
        gpui::FontWeight::NORMAL
    };
    let styled = div().font_weight(weight).text_color(theme.text);
    let styled = if run.italic { styled.italic() } else { styled };
    match run.link {
        None => styled.child(run.text.to_string()).into_any_element(),
        Some(url) => {
            let n = *counter;
            *counter += 1;
            let selector = format!("email-body-link-{index}-{n}");
            let target = url.to_string();
            let opener = target.clone();
            styled
                .id(selector.clone())
                .debug_selector(move || selector.clone())
                .text_color(theme.accent)
                .cursor_pointer()
                .aria_label(format!("Open link {target}"))
                .on_click(move |_, _, cx| {
                    cx.open_url(&opener);
                })
                .child(run.text.to_string())
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EmailId, Mailbox, Origin};
    use gpui::{AppContext, Context, Render, TestAppContext, VisualTestContext};

    fn text(value: &str) -> BodySegment<'_> {
        BodySegment::Text(value)
    }

    fn link(value: &str) -> BodySegment<'_> {
        BodySegment::Link(value)
    }

    #[test]
    fn a_bare_url_becomes_one_link() {
        assert_eq!(
            split_links("Shop https://shop.example/e today"),
            vec![
                text("Shop "),
                link("https://shop.example/e"),
                text(" today"),
            ]
        );
    }

    #[test]
    fn an_angle_wrapped_url_is_one_link_not_two() {
        assert_eq!(
            split_links("See <https://shop.example/e> today"),
            vec![text("See "), link("https://shop.example/e"), text(" today"),]
        );
    }

    #[test]
    fn trailing_prose_punctuation_stays_outside_the_link() {
        assert_eq!(
            split_links("Visit https://example.com/a, or https://example.com/b."),
            vec![
                text("Visit "),
                link("https://example.com/a"),
                text(", or "),
                link("https://example.com/b"),
                text("."),
            ]
        );
    }

    #[test]
    fn balanced_parens_survive_inside_a_link() {
        assert_eq!(
            split_links("(see https://en.wikipedia.org/wiki/X_(thing))"),
            vec![
                text("(see "),
                link("https://en.wikipedia.org/wiki/X_(thing)"),
                text(")"),
            ]
        );
    }

    #[test]
    fn a_bare_scheme_with_nothing_after_it_is_text() {
        assert_eq!(
            split_links("visit https:// today"),
            vec![text("visit https:// today")]
        );
    }

    #[test]
    fn text_without_a_scheme_stays_whole() {
        assert_eq!(
            split_links("No links here <bob@example.com>"),
            vec![text("No links here <bob@example.com>")]
        );
    }

    fn linked_email() -> Email {
        Email {
            id: EmailId::from(9001u16),
            sender: "Green Man Gaming".to_string(),
            address: "deals@example.com".to_string(),
            recipients: vec!["me@example.com".to_string()],
            subject: "Over 75% off".to_string(),
            preview: "Save on 1,000+ games".to_string(),
            body: nori_gmail::text_blocks(vec![
                "Shop the sale <https://shop.example/e> today".to_string(),
                "Or browse https://example.com/games, while it lasts".to_string(),
            ]),
            timestamp: "Sep 26".to_string(),
            full_date: "Sep 26".to_string(),
            mailbox: Mailbox::Inbox,
            unread: false,
            starred: false,
            pinned: false,
            body_loaded: true,
            origin: Origin::Sample,
        }
    }

    struct LinkHost;

    impl Render for LinkHost {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let focus = cx.focus_handle();
            EmailView::new(
                linked_email(),
                focus,
                std::collections::HashMap::new(),
                |_, _| {},
                |_, _| {},
                |_, _| {},
                |_, _| {},
            )
        }
    }

    #[gpui::test]
    fn body_links_render_as_click_targets(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_window, cx| cx.new(|_| LinkHost))
                .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("email-body-link-0-0").is_some(),
            "the wrapped URL in the first paragraph must render as a link"
        );
        assert!(
            cx.debug_bounds("email-body-link-1-0").is_some(),
            "the bare URL in the second paragraph must render as a link"
        );
    }

    fn rich_email() -> Email {
        let mut mail = linked_email();
        mail.body = nori_gmail::parse_html_body(concat!(
            "<h1>Tokyo sale</h1>",
            "<p>Up to <b>75% off</b> <a href=\"https://shop.example/e\">here</a></p>",
            "<ul><li>One</li><li>Two</li></ul>",
            "<img src=\"https://img.example/hero.jpg\" alt=\"Hero art\">",
        ));
        mail
    }

    struct RichHost;

    impl Render for RichHost {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let focus = cx.focus_handle();
            EmailView::new(
                rich_email(),
                focus,
                std::collections::HashMap::new(),
                |_, _| {},
                |_, _| {},
                |_, _| {},
                |_, _| {},
            )
        }
    }

    #[gpui::test]
    fn rich_blocks_render_structure_links_and_image_placeholders(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_window, cx| cx.new(|_| RichHost))
                .unwrap()
        });
        let mut cx = VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        // Blocks: heading 0, paragraph 1, list 2, image 3.
        assert!(
            cx.debug_bounds("email-body-link-1-0").is_some(),
            "the shop link in the paragraph must render as a link"
        );
        assert!(
            cx.debug_bounds("email-body-img-3").is_some(),
            "the hero must render, unfetched, as its alt-text placeholder"
        );
    }
}
