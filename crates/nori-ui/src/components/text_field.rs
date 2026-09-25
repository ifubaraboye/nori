use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, IntoElement, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    Render, SharedString, Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
    actions, div, fill, point, prelude::*, px, relative, rgba, size,
};
use unicode_segmentation::UnicodeSegmentation;

actions!(
    nori_text_field,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        Home,
        End
    ]
);

pub(crate) fn register_bindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("NoriTextField")),
        KeyBinding::new("delete", Delete, Some("NoriTextField")),
        KeyBinding::new("left", Left, Some("NoriTextField")),
        KeyBinding::new("right", Right, Some("NoriTextField")),
        KeyBinding::new("shift-left", SelectLeft, Some("NoriTextField")),
        KeyBinding::new("shift-right", SelectRight, Some("NoriTextField")),
        KeyBinding::new("home", Home, Some("NoriTextField")),
        KeyBinding::new("end", End, Some("NoriTextField")),
    ]);
}

pub struct TextField {
    id: SharedString,
    placeholder: SharedString,
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    focus_handle: FocusHandle,
    single_line: bool,
    tab_index: isize,
    last_lines: Vec<gpui::WrappedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    undo_stack: Vec<UndoSnapshot>,
    redo_stack: Vec<UndoSnapshot>,
}

/// One undo step: the full field state before a committed edit. Fields are
/// small single-purpose inputs, so whole-content snapshots stay cheap.
#[derive(Clone)]
struct UndoSnapshot {
    content: String,
    cursor: usize,
}

/// Cap history so long-lived compose fields cannot grow it without bound.
const MAX_UNDO_ENTRIES: usize = 100;

impl TextField {
    pub fn new(
        id: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        content: impl Into<String>,
        single_line: bool,
        tab_index: isize,
        cx: &mut Context<Self>,
    ) -> Self {
        let content = content.into();
        let selected_range = content.len()..content.len();
        Self {
            id: id.into(),
            placeholder: placeholder.into(),
            content,
            selected_range,
            selection_reversed: false,
            marked_range: None,
            focus_handle: cx.focus_handle().tab_index(tab_index).tab_stop(true),
            single_line,
            tab_index,
            last_lines: Vec::new(),
            last_bounds: None,
            is_selecting: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    /// Replace the whole content, used to clear a field after a commit.
    pub fn set_content(&mut self, content: impl Into<String>, cx: &mut Context<Self>) {
        let content = content.into();
        let cursor = content.len();
        self.content = content;
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = clamp_boundary(&self.content, offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = clamp_boundary(&self.content, offset);
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selected_range = self.selected_range.end..self.selected_range.start;
            self.selection_reversed = !self.selection_reversed;
        }
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn left(&mut self, _: &Left, _window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let offset = self.previous_boundary(self.cursor_offset());
            self.move_to(offset, cx);
        } else {
            let offset = self.selected_range.start;
            self.move_to(offset, cx);
        }
    }

    fn right(&mut self, _: &Right, _window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let offset = self.next_boundary(self.cursor_offset());
            self.move_to(offset, cx);
        } else {
            let offset = self.selected_range.end;
            self.move_to(offset, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _window: &mut Window, cx: &mut Context<Self>) {
        let offset = self.previous_boundary(self.cursor_offset());
        self.select_to(offset, cx);
    }

    fn select_right(&mut self, _: &SelectRight, _window: &mut Window, cx: &mut Context<Self>) {
        let offset = self.next_boundary(self.cursor_offset());
        self.select_to(offset, cx);
    }

    fn select_all(
        &mut self,
        _: &crate::input::SelectAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _window: &mut Window, cx: &mut Context<Self>) {
        let offset = self.content[..clamp_boundary(&self.content, self.cursor_offset())]
            .rfind('\n')
            .map(|index| index + 1)
            .unwrap_or(0);
        self.move_to(offset, cx);
    }

    fn end(&mut self, _: &End, _window: &mut Window, cx: &mut Context<Self>) {
        let offset = self.cursor_offset();
        let end = self.content[offset..]
            .find('\n')
            .map(|index| offset + index)
            .unwrap_or(self.content.len());
        self.move_to(end, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let previous = self.previous_boundary(self.cursor_offset());
            if previous == self.cursor_offset() {
                window.play_system_bell();
                return;
            }
            self.select_to(previous, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if next == self.cursor_offset() {
                window.play_system_bell();
                return;
            }
            self.select_to(next, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &crate::input::Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let text = if self.single_line {
                text.replace(['\n', '\r'], " ")
            } else {
                text
            };
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &crate::input::Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &crate::input::Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    /// Snapshot the pre-edit state for undo. Called by every committed edit
    /// path; undo/redo restore directly so they never record themselves, and
    /// IME composition (`replace_and_mark_text_in_range`) records nothing
    /// until the platform commits it through the plain replace path.
    fn push_history(&mut self) {
        self.undo_stack.push(UndoSnapshot {
            content: self.content.clone(),
            cursor: self.cursor_offset(),
        });
        if self.undo_stack.len() > MAX_UNDO_ENTRIES {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn undo(&mut self, _: &crate::input::Undo, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(previous) = self.undo_stack.pop() else {
            return;
        };
        self.redo_stack.push(UndoSnapshot {
            content: std::mem::take(&mut self.content),
            cursor: self.cursor_offset(),
        });
        self.content = previous.content;
        let cursor = clamp_boundary(&self.content, previous.cursor);
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn redo(&mut self, _: &crate::input::Redo, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(next) = self.redo_stack.pop() else {
            return;
        };
        self.undo_stack.push(UndoSnapshot {
            content: std::mem::take(&mut self.content),
            cursor: self.cursor_offset(),
        });
        if self.undo_stack.len() > MAX_UNDO_ENTRIES {
            self.undo_stack.remove(0);
        }
        self.content = next.content;
        let cursor = clamp_boundary(&self.content, next.cursor);
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.is_selecting = true;
        let index = self.index_for_point(event.position);
        if event.modifiers.shift {
            self.select_to(index, cx);
        } else {
            self.move_to(index, cx);
        }
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting {
            self.select_to(self.index_for_point(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn index_for_point(&self, point: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        if point.y < bounds.top() {
            return 0;
        }
        let line_height = px(20.);
        let line_index = ((point.y - bounds.top()) / line_height).floor().max(0.) as usize;
        let line_index = line_index.min(self.last_lines.len().saturating_sub(1));
        let Some(line) = self.last_lines.get(line_index) else {
            return 0;
        };
        let local = line
            .unwrapped_layout
            .closest_index_for_x((point.x - bounds.left()).max(px(0.)));
        self.line_start(line_index) + local
    }

    fn line_start(&self, line_index: usize) -> usize {
        self.content
            .split_inclusive('\n')
            .take(line_index)
            .map(str::len)
            .sum()
    }

    fn line_index_for_offset(&self, offset: usize) -> (usize, usize) {
        let offset = clamp_boundary(&self.content, offset);
        let line_index = self.content[..offset].matches('\n').count();
        let start = self.line_start(line_index);
        (line_index, offset - start)
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let offset = clamp_boundary(&self.content, offset);
        self.content[..offset].chars().map(char::len_utf16).sum()
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let range = clamp_range(&self.content, range);
        let new_text = if self.single_line {
            new_text.replace(['\n', '\r'], " ")
        } else {
            new_text.to_string()
        };
        self.push_history();
        self.content.replace_range(range.clone(), &new_text);
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.marked_range = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let range = clamp_range(&self.content, range);
        let new_text = if self.single_line {
            new_text.replace(['\n', '\r'], " ")
        } else {
            new_text.to_string()
        };
        self.content.replace_range(range.clone(), &new_text);
        if new_text.is_empty() {
            self.marked_range = None;
        } else {
            self.marked_range = Some(range.start..range.start + new_text.len());
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|selected| {
                let selected = self.range_from_utf16(selected);
                selected.start + range.start..selected.end + range.start
            })
            .unwrap_or_else(|| {
                let cursor = range.start + new_text.len();
                cursor..cursor
            });
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let bounds = self.last_bounds?;
        let range = self.range_from_utf16(&range_utf16);
        let (line_index, local) = self.line_index_for_offset(range.start);
        let line = self.last_lines.get(line_index)?;
        let x = bounds.left() + line.unwrapped_layout.x_for_index(local);
        Some(Bounds::new(
            point(x, bounds.top() + px(20.) * line_index as f32),
            size(px(2.), px(20.)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let line_index = ((point.y - bounds.top()) / px(20.)).floor().max(0.) as usize;
        let line_index = line_index.min(self.last_lines.len().saturating_sub(1));
        let line = self.last_lines.get(line_index)?;
        let local = line
            .unwrapped_layout
            .closest_index_for_x((point.x - bounds.left()).max(px(0.)));
        Some(self.line_start(line_index) + local)
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(self.id.clone())
            .key_context("NoriTextField")
            .track_focus(&self.focus_handle)
            .tab_index(self.tab_index)
            .h(if self.single_line { px(28.) } else { px(132.) })
            .w_full()
            .flex_none()
            .cursor(CursorStyle::IBeam)
            .text_size(px(13.))
            .line_height(px(20.))
            .text_color(crate::theme::Theme::dark().text)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::select_all))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(TextElement {
                input: cx.entity(),
                placeholder: self.placeholder.clone(),
                theme: crate::theme::Theme::dark(),
                single_line: self.single_line,
            })
    }
}

struct TextElement {
    input: Entity<TextField>,
    placeholder: SharedString,
    theme: crate::theme::Theme,
    single_line: bool,
}

struct TextPrepaint {
    lines: Vec<gpui::WrappedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = TextPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = if self.single_line {
            px(28.).into()
        } else {
            relative(1.).into()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let is_placeholder = input.content.is_empty();
        let display_text = if is_placeholder {
            self.placeholder.clone()
        } else {
            SharedString::new(input.content.clone())
        };
        let style = window.text_style();
        let color = if is_placeholder {
            self.theme.faint
        } else {
            self.theme.text
        };
        let base_run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked.start,
                    ..base_run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(color),
                        thickness: px(1.),
                        wavy: false,
                    }),
                    ..base_run.clone()
                },
                TextRun {
                    len: display_text.len() - marked.end,
                    ..base_run.clone()
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect::<Vec<_>>()
        } else {
            vec![base_run]
        };
        let wrap_width = if self.single_line {
            None
        } else {
            Some(bounds.size.width)
        };
        let lines = window
            .text_system()
            .shape_text(display_text, px(13.), &runs, wrap_width, None)
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();

        let mut cursor = None;
        let mut selection = None;
        if !is_placeholder && input.selected_range.start != input.selected_range.end {
            let (start_line, start_local) = input.line_index_for_offset(input.selected_range.start);
            let (end_line, end_local) = input.line_index_for_offset(input.selected_range.end);
            if start_line == end_line && lines.get(start_line).is_some() {
                let line = &lines[start_line];
                let x1 = bounds.left() + line.unwrapped_layout.x_for_index(start_local);
                let x2 = bounds.left() + line.unwrapped_layout.x_for_index(end_local);
                selection = Some(fill(
                    Bounds::new(
                        point(x1, bounds.top() + px(20.) * start_line as f32),
                        size(x2 - x1, px(20.)),
                    ),
                    rgba(0x4e769522),
                ));
            }
        }
        if !is_placeholder
            && input.focus_handle.is_focused(window)
            && input.selected_range.is_empty()
        {
            let (line_index, local) = input.line_index_for_offset(input.cursor_offset());
            if let Some(line) = lines.get(line_index) {
                let x = bounds.left() + line.unwrapped_layout.x_for_index(local);
                cursor = Some(fill(
                    Bounds::new(
                        point(x, bounds.top() + px(20.) * line_index as f32),
                        size(px(1.5), px(20.)),
                    ),
                    self.theme.text,
                ));
            }
        }
        TextPrepaint {
            lines,
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let lines = std::mem::take(&mut prepaint.lines);
        for (index, line) in lines.iter().enumerate() {
            let line_bounds = Bounds::new(
                point(bounds.left(), bounds.top() + px(20.) * index as f32),
                size(bounds.size.width, px(20.)),
            );
            let _ = line.paint(
                point(line_bounds.left(), line_bounds.top()),
                px(20.),
                TextAlign::Left,
                Some(line_bounds),
                window,
                cx,
            );
        }
        if focus_handle.is_focused(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _cx| {
            input.last_lines = lines;
            input.last_bounds = Some(bounds);
        });
    }
}

fn clamp_boundary(content: &str, mut offset: usize) -> usize {
    offset = offset.min(content.len());
    while offset > 0 && !content.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn clamp_range(content: &str, range: Range<usize>) -> Range<usize> {
    clamp_boundary(content, range.start)..clamp_boundary(content, range.end.max(range.start))
}
