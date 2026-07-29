use std::{cell::RefCell, collections::BTreeSet, ops::Range, rc::Rc};

use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, SharedString, StrikethroughStyle,
    Style, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, div, fill,
    point, prelude::*, px, relative, rgba, size,
};
use gpui_component::input::{
    Backspace, Copy, Cut, Delete, DeleteToBeginningOfLine, DeleteToEndOfLine, DeleteToNextWordEnd,
    DeleteToPreviousWordStart, Enter, MoveDown, MoveLeft, MoveRight, MoveToEndOfLine,
    MoveToNextWord, MoveToPreviousWord, MoveToStartOfLine, MoveUp, Paste, SelectAll,
    SelectToEndOfLine, SelectToNextWordEnd, SelectToPreviousWordStart, SelectToStartOfLine,
    ShowCharacterPalette,
};
use ideall_application::{
    NoteColor, NoteInlineDocument, NoteInlineMark, NoteInlineStyle, note_mention_query,
};
use unicode_segmentation::UnicodeSegmentation as _;

use super::{ACCENT, ACCENT_SOFT, INK_FAINT};

const RICH_CLIPBOARD_METADATA_PREFIX: &str = "ideall-rich-markdown-v1:";

#[derive(Clone, Debug)]
pub(super) enum RichInputEvent {
    Change,
    SelectionChange,
    OpenLink(String),
    PressEnter { secondary: bool },
    BackspaceAtStart,
    Navigate(RichNavigation),
}

#[derive(Clone, Copy, Debug)]
pub(super) enum RichNavigation {
    Previous,
    Next,
}

pub(super) struct RichInputState {
    focus_handle: FocusHandle,
    document: NoteInlineDocument,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    pending_marks: BTreeSet<NoteInlineMark>,
    read_only: bool,
    last_layout: Option<RichLayout>,
    last_bounds: Option<Bounds<Pixels>>,
    last_line_height: Pixels,
    is_selecting: bool,
}

impl RichInputState {
    pub(super) fn new(
        markdown: impl AsRef<str>,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            document: NoteInlineDocument::from_markdown(markdown.as_ref()),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            pending_marks: BTreeSet::new(),
            read_only: false,
            last_layout: None,
            last_bounds: None,
            last_line_height: Pixels::ZERO,
            is_selecting: false,
        }
    }

    pub(super) fn text(&self) -> &str {
        &self.document.text
    }

    pub(super) fn markdown(&self) -> String {
        self.document.to_markdown()
    }

    pub(super) fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    pub(super) fn has_selection(&self) -> bool {
        !self.selected_range.is_empty()
    }

    pub(super) fn selection_has_style(&self, style: NoteInlineStyle) -> bool {
        let matches_style = |mark: &NoteInlineMark| match style {
            NoteInlineStyle::Bold => matches!(mark, NoteInlineMark::Bold),
            NoteInlineStyle::Italic => matches!(mark, NoteInlineMark::Italic),
            NoteInlineStyle::Strikethrough => matches!(mark, NoteInlineMark::Strikethrough),
            NoteInlineStyle::Code => matches!(mark, NoteInlineMark::Code),
            NoteInlineStyle::Link => matches!(mark, NoteInlineMark::Link(_)),
        };
        if self.selected_range.is_empty() {
            return self.pending_marks.iter().any(matches_style);
        }
        self.document.spans.iter().any(|span| {
            span.range.start <= self.selected_range.start
                && span.range.end >= self.selected_range.end
                && matches_style(&span.mark)
        })
    }

    pub(super) fn selection_color(&self, foreground: bool) -> Option<NoteColor> {
        let color_for_mark = |mark: &NoteInlineMark| match (foreground, mark) {
            (true, NoteInlineMark::TextColor(color))
            | (false, NoteInlineMark::Highlight(color)) => Some(*color),
            _ => None,
        };
        if self.selected_range.is_empty() {
            return self.pending_marks.iter().find_map(color_for_mark);
        }
        self.document.spans.iter().find_map(|span| {
            (span.range.start <= self.selected_range.start
                && span.range.end >= self.selected_range.end)
                .then(|| color_for_mark(&span.mark))
                .flatten()
        })
    }

    pub(super) fn link_target(&self) -> Option<(Range<usize>, Option<String>)> {
        if self.selected_range.is_empty() {
            let cursor = self.cursor_offset();
            return self.document.spans.iter().find_map(|span| {
                if span.range.start <= cursor
                    && cursor <= span.range.end
                    && let NoteInlineMark::Link(url) = &span.mark
                {
                    Some((span.range.clone(), Some(url.clone())))
                } else {
                    None
                }
            });
        }
        let url = self.document.spans.iter().find_map(|span| {
            if span.range.start <= self.selected_range.start
                && span.range.end >= self.selected_range.end
                && let NoteInlineMark::Link(url) = &span.mark
            {
                Some(url.clone())
            } else {
                None
            }
        });
        Some((self.selected_range.clone(), url))
    }

    pub(super) fn mention_query(&self) -> Option<(Range<usize>, String)> {
        let (range, query) = note_mention_query(&self.document.text, self.cursor_offset())?;
        let already_linked = self.document.spans.iter().any(|span| {
            matches!(span.mark, NoteInlineMark::Link(_))
                && span.range.start <= range.start
                && span.range.end >= range.end
        });
        (!already_linked).then(|| (range, query.to_owned()))
    }

    pub(super) fn apply_link(
        &mut self,
        range: Range<usize>,
        url: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.document.set_link(range.clone(), url);
        if changed {
            self.selected_range = range;
            self.selection_reversed = false;
            self.pending_marks = self.document.marks_at(self.cursor_offset());
            cx.emit(RichInputEvent::Change);
            cx.notify();
        }
        changed
    }

    pub(super) fn remove_link(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> bool {
        let changed = self.document.remove_links(range.clone());
        if changed {
            self.selected_range = range;
            self.selection_reversed = false;
            self.pending_marks = self.document.marks_at(self.cursor_offset());
            cx.emit(RichInputEvent::Change);
            cx.notify();
        }
        changed
    }

    pub(super) fn insert_link(
        &mut self,
        range: Range<usize>,
        label: &str,
        url: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(cursor) = self.document.replace_with_link(range, label, url) else {
            return false;
        };
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.pending_marks = self.document.marks_at(cursor);
        cx.emit(RichInputEvent::Change);
        cx.notify();
        true
    }

    pub(super) fn split_markdown_at_cursor(&self) -> (String, String) {
        let cursor = self.cursor_offset();
        (
            self.document.slice(0..cursor).to_markdown(),
            self.document
                .slice(cursor..self.document.text.len())
                .to_markdown(),
        )
    }

    pub(super) fn append_markdown(
        &mut self,
        markdown: impl AsRef<str>,
        cx: &mut Context<Self>,
    ) -> usize {
        let join_offset = self.document.text.len();
        self.document
            .append(&NoteInlineDocument::from_markdown(markdown.as_ref()));
        self.move_to(join_offset, cx);
        cx.emit(RichInputEvent::Change);
        cx.notify();
        join_offset
    }

    pub(super) fn set_markdown(&mut self, markdown: impl AsRef<str>, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        self.document = NoteInlineDocument::from_markdown(markdown.as_ref());
        self.move_to(cursor.min(self.document.text.len()), cx);
        self.marked_range = None;
        self.pending_marks = self.document.marks_at(self.cursor_offset());
        cx.notify();
    }

    pub(super) fn set_cursor_offset(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.move_to(self.snap_offset(offset), cx);
        self.pending_marks = self.document.marks_at(self.cursor_offset());
    }

    pub(super) fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    pub(super) fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only == read_only {
            return;
        }
        self.read_only = read_only;
        self.marked_range = None;
        if read_only {
            self.pending_marks.clear();
        } else {
            self.pending_marks = self.document.marks_at(self.cursor_offset());
        }
        cx.notify();
    }

    pub(super) fn toggle_style(&mut self, style: NoteInlineStyle, cx: &mut Context<Self>) -> bool {
        if style == NoteInlineStyle::Link {
            if self.selected_range.is_empty() {
                return false;
            }
            let selected_link = self.document.spans.iter().find_map(|span| {
                if span.range.start <= self.selected_range.start
                    && span.range.end >= self.selected_range.end
                    && let NoteInlineMark::Link(url) = &span.mark
                {
                    Some(NoteInlineMark::Link(url.clone()))
                } else {
                    None
                }
            });
            let Some(mark) = selected_link else {
                return false;
            };
            let changed = self.document.toggle_mark(self.selected_range.clone(), mark);
            if changed {
                self.pending_marks = self.document.marks_at(self.cursor_offset());
                cx.emit(RichInputEvent::Change);
                cx.notify();
            }
            return changed;
        }
        let mark = match style {
            NoteInlineStyle::Bold => NoteInlineMark::Bold,
            NoteInlineStyle::Italic => NoteInlineMark::Italic,
            NoteInlineStyle::Strikethrough => NoteInlineMark::Strikethrough,
            NoteInlineStyle::Code => NoteInlineMark::Code,
            NoteInlineStyle::Link => unreachable!("link handling returns above"),
        };
        if self.selected_range.is_empty() {
            if self.pending_marks.contains(&mark) {
                self.pending_marks.remove(&mark);
            } else {
                self.pending_marks.insert(mark);
            }
            cx.notify();
            return true;
        }
        let changed = self.document.toggle_mark(self.selected_range.clone(), mark);
        if changed {
            self.pending_marks = self.document.marks_at(self.cursor_offset());
            cx.emit(RichInputEvent::Change);
            cx.notify();
        }
        changed
    }

    pub(super) fn set_color(
        &mut self,
        foreground: bool,
        color: Option<NoteColor>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.selected_range.is_empty() {
            self.pending_marks.retain(|mark| {
                if foreground {
                    !matches!(mark, NoteInlineMark::TextColor(_))
                } else {
                    !matches!(mark, NoteInlineMark::Highlight(_))
                }
            });
            if let Some(color) = color {
                self.pending_marks.insert(if foreground {
                    NoteInlineMark::TextColor(color)
                } else {
                    NoteInlineMark::Highlight(color)
                });
            }
            cx.notify();
            return true;
        }
        let changed = if foreground {
            self.document
                .set_text_color(self.selected_range.clone(), color)
        } else {
            self.document
                .set_highlight(self.selected_range.clone(), color)
        };
        if changed {
            self.pending_marks = self.document.marks_at(self.cursor_offset());
            cx.emit(RichInputEvent::Change);
            cx.notify();
        }
        changed
    }

    pub(super) fn replace_all(
        &mut self,
        query: &str,
        replacement: &str,
        cx: &mut Context<Self>,
    ) -> usize {
        if query.is_empty() {
            return 0;
        }
        let ranges = self
            .document
            .text
            .match_indices(query)
            .map(|(start, value)| start..start + value.len())
            .collect::<Vec<_>>();
        for range in ranges.iter().rev() {
            let inherited = self.document.marks_at(range.start);
            self.document
                .replace(range.clone(), replacement, &inherited);
        }
        if !ranges.is_empty() {
            self.move_to(self.cursor_offset().min(self.document.text.len()), cx);
            cx.emit(RichInputEvent::Change);
            cx.notify();
        }
        ranges.len()
    }

    fn move_left(&mut self, _: &MoveLeft, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            if self.cursor_offset() == 0 {
                cx.emit(RichInputEvent::Navigate(RichNavigation::Previous));
                return;
            }
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn move_right(&mut self, _: &MoveRight, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            if self.cursor_offset() == self.document.text.len() {
                cx.emit(RichInputEvent::Navigate(RichNavigation::Next));
                return;
            }
            self.move_to(self.next_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn move_to_previous_word(
        &mut self,
        _: &MoveToPreviousWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_to(self.previous_word_boundary(self.cursor_offset()), cx);
    }

    fn move_to_next_word(&mut self, _: &MoveToNextWord, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_word_boundary(self.cursor_offset()), cx);
    }

    fn select_to_previous_word(
        &mut self,
        _: &SelectToPreviousWordStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.previous_word_boundary(self.cursor_offset()), cx);
    }

    fn select_to_next_word(
        &mut self,
        _: &SelectToNextWordEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.next_word_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.document.text.len(), cx);
    }

    fn move_to_start(&mut self, _: &MoveToStartOfLine, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn move_to_end(&mut self, _: &MoveToEndOfLine, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.document.text.len(), cx);
    }

    fn select_to_start(&mut self, _: &SelectToStartOfLine, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEndOfLine, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.document.text.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            if self.cursor_offset() == 0 {
                cx.emit(RichInputEvent::BackspaceAtStart);
                return;
            }
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_to_start(
        &mut self,
        _: &DeleteToBeginningOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(0, cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_to_end(
        &mut self,
        _: &DeleteToEndOfLine,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.document.text.len(), cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_previous_word(
        &mut self,
        _: &DeleteToPreviousWordStart,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.previous_word_boundary(self.cursor_offset()), cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_next_word(
        &mut self,
        _: &DeleteToNextWordEnd,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_to(self.next_word_boundary(self.cursor_offset()), cx);
        self.replace_text_in_range(None, "", window, cx);
    }

    fn press_enter(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        if action.secondary {
            self.replace_text_in_range(None, "\n", window, cx);
            return;
        }
        cx.emit(RichInputEvent::PressEnter { secondary: false });
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.vertical_offset(false) {
            self.move_to(offset, cx);
        } else {
            cx.emit(RichInputEvent::Navigate(RichNavigation::Previous));
        }
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(offset) = self.vertical_offset(true) {
            self.move_to(offset, cx);
        } else {
            cx.emit(RichInputEvent::Navigate(RichNavigation::Next));
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if let Some(markdown) = item
            .metadata()
            .and_then(|metadata| metadata.strip_prefix(RICH_CLIPBOARD_METADATA_PREFIX))
        {
            let fragment = NoteInlineDocument::from_markdown(markdown);
            let cursor = self
                .document
                .replace_document(self.selected_range.clone(), &fragment);
            self.selected_range = cursor..cursor;
            self.selection_reversed = false;
            self.marked_range = None;
            self.pending_marks = self.document.marks_at(cursor);
            cx.emit(RichInputEvent::Change);
            cx.notify();
            return;
        }
        if let Some(text) = item.text() {
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            let plain = self.document.text[self.selected_range.clone()].to_owned();
            let markdown = self
                .document
                .slice(self.selected_range.clone())
                .to_markdown();
            cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(
                plain,
                format!("{RICH_CLIPBOARD_METADATA_PREFIX}{markdown}"),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        self.copy(&Copy, window, cx);
        if !self.selected_range.is_empty() {
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let offset = self.index_for_mouse_position(event.position);
        if event.click_count >= 2
            && let Some(url) = self.document.spans.iter().find_map(|span| {
                if span.range.start <= offset
                    && offset <= span.range.end
                    && let NoteInlineMark::Link(url) = &span.mark
                {
                    Some(url.clone())
                } else {
                    None
                }
            })
        {
            self.is_selecting = false;
            cx.emit(RichInputEvent::OpenLink(url));
            cx.notify();
            return;
        }
        if event.modifiers.shift {
            self.select_to(offset, cx);
        } else {
            self.move_to(offset, cx);
        }
        self.pending_marks = self.document.marks_at(self.cursor_offset());
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if !modifiers.shift || modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        if matches!(event.keystroke.key.as_str(), "enter" | "return") {
            self.replace_text_in_range(None, "\n", window, cx);
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        match event.keystroke.key.as_str() {
            "left" => self.select_to(self.previous_boundary(self.cursor_offset()), cx),
            "right" => self.select_to(self.next_boundary(self.cursor_offset()), cx),
            _ => return,
        }
        window.prevent_default();
        cx.stop_propagation();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.snap_offset(offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.emit(RichInputEvent::SelectionChange);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let offset = self.snap_offset(offset);
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.emit(RichInputEvent::SelectionChange);
        cx.notify();
    }

    fn snap_offset(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.document.text.len());
        while !self.document.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.document
            .text
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.document
            .text
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.document.text.len())
    }

    fn previous_word_boundary(&self, offset: usize) -> usize {
        let before = &self.document.text[..self.snap_offset(offset)];
        let trimmed = before.trim_end_matches(|character: char| {
            character.is_whitespace() || character.is_ascii_punctuation()
        });
        trimmed
            .char_indices()
            .rev()
            .find_map(|(index, character)| {
                (character.is_whitespace() || character.is_ascii_punctuation())
                    .then_some(index + character.len_utf8())
            })
            .unwrap_or(0)
    }

    fn next_word_boundary(&self, offset: usize) -> usize {
        let offset = self.snap_offset(offset);
        let after = &self.document.text[offset..];
        let mut seen_word = false;
        for (index, character) in after.char_indices() {
            let separator = character.is_whitespace() || character.is_ascii_punctuation();
            if seen_word && separator {
                return offset + index;
            }
            seen_word |= !separator;
        }
        self.document.text.len()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for character in self.document.text.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += character.len_utf16();
            utf8_offset += character.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for character in self.document.text.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += character.len_utf8();
            utf16_offset += character.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.document.text.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(layout)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.document.text.len();
        }
        let local = position - bounds.origin;
        layout
            .closest_index_for_position(local, self.last_line_height)
            .unwrap_or_default()
    }

    fn vertical_offset(&self, down: bool) -> Option<usize> {
        let layout = self.last_layout.as_ref()?;
        if self.last_line_height <= Pixels::ZERO {
            return None;
        }
        let cursor = self.cursor_offset();
        let position = layout.position_for_index(cursor, self.last_line_height)?;
        let target_y = if down {
            position.y + self.last_line_height
        } else {
            if position.y < self.last_line_height {
                return None;
            }
            position.y - self.last_line_height
        };
        if target_y >= layout.height {
            return None;
        }
        layout.closest_index_for_position(
            point(position.x, target_y + self.last_line_height / 2.0),
            self.last_line_height,
        )
    }
}

impl EventEmitter<RichInputEvent> for RichInputState {}

impl Focusable for RichInputState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for RichInputState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.document.text[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let mut inherited = self.document.marks_at(range.start);
        inherited.extend(self.pending_marks.iter().cloned());
        self.document.replace(range.clone(), text, &inherited);
        let cursor = range.start + text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.emit(RichInputEvent::Change);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        selected_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let mut inherited = self.document.marks_at(range.start);
        inherited.extend(self.pending_marks.iter().cloned());
        self.document.replace(range.clone(), text, &inherited);
        self.marked_range = (!text.is_empty()).then_some(range.start..range.start + text.len());
        self.selected_range = selected_utf16
            .as_ref()
            .map(|selected| self.range_from_utf16(selected))
            .map(|selected| range.start + selected.start..range.start + selected.end)
            .unwrap_or_else(|| {
                let cursor = range.start + text.len();
                cursor..cursor
            });
        self.selection_reversed = false;
        cx.emit(RichInputEvent::Change);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let start = layout.position_for_index(range.start, self.last_line_height)?;
        let end = layout.position_for_index(range.end, self.last_line_height)?;
        Some(Bounds::from_corners(
            bounds.origin + start,
            point(
                bounds.left() + end.x,
                bounds.top() + end.y + self.last_line_height,
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds.as_ref()?;
        let layout = self.last_layout.as_ref()?;
        let local = position - bounds.origin;
        let index = layout.closest_index_for_position(local, self.last_line_height)?;
        Some(self.offset_to_utf16(index))
    }
}

impl Render for RichInputState {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .key_context("Input")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_to_start))
            .on_action(cx.listener(Self::delete_to_end))
            .on_action(cx.listener(Self::delete_previous_word))
            .on_action(cx.listener(Self::delete_next_word))
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::move_to_previous_word))
            .on_action(cx.listener(Self::move_to_next_word))
            .on_action(cx.listener(Self::select_to_previous_word))
            .on_action(cx.listener(Self::select_to_next_word))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::move_to_start))
            .on_action(cx.listener(Self::move_to_end))
            .on_action(cx.listener(Self::select_to_start))
            .on_action(cx.listener(Self::select_to_end))
            .on_action(cx.listener(Self::press_enter))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(RichTextElement { input: cx.entity() })
    }
}

struct RichTextElement {
    input: Entity<RichInputState>,
}

struct RichPrepaintState {
    layout: Option<RichLayout>,
    line_height: Pixels,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
}

#[derive(Clone)]
struct RichMeasuredLayout {
    layout: RichLayout,
    line_height: Pixels,
}

#[derive(Clone)]
struct RichLayout {
    lines: Vec<RichLayoutLine>,
    height: Pixels,
}

#[derive(Clone)]
struct RichLayoutLine {
    range: Range<usize>,
    line: WrappedLine,
    y: Pixels,
    height: Pixels,
}

impl RichLayout {
    fn position_for_index(&self, index: usize, line_height: Pixels) -> Option<Point<Pixels>> {
        let line = self
            .lines
            .iter()
            .rev()
            .find(|line| line.range.start <= index)?;
        let local = index
            .saturating_sub(line.range.start)
            .min(line.range.end.saturating_sub(line.range.start));
        let position = line.line.position_for_index(local, line_height)?;
        Some(point(position.x, line.y + position.y))
    }

    fn closest_index_for_position(
        &self,
        position: Point<Pixels>,
        line_height: Pixels,
    ) -> Option<usize> {
        let line = self
            .lines
            .iter()
            .find(|line| position.y < line.y + line.height)
            .or_else(|| self.lines.last())?;
        let local = point(position.x, (position.y - line.y).max(Pixels::ZERO));
        let index = line
            .line
            .closest_index_for_position(local, line_height)
            .unwrap_or_else(|index| index)
            .min(line.range.end.saturating_sub(line.range.start));
        Some(line.range.start + index)
    }
}

impl IntoElement for RichTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for RichTextElement {
    type RequestLayoutState = Rc<RefCell<Option<RichMeasuredLayout>>>;
    type PrepaintState = RichPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        let input = self.input.read(cx);
        let text_style = window.text_style();
        let content_empty = input.document.text.is_empty();
        let line_specs = if content_empty {
            let display_text = input.placeholder.clone();
            vec![(
                0..0,
                display_text.clone(),
                rich_text_runs(input, &display_text, true, &text_style, 0),
            )]
        } else {
            rich_text_line_ranges(&input.document.text)
                .into_iter()
                .map(|range| {
                    let display_text: SharedString =
                        input.document.text[range.clone()].to_owned().into();
                    let runs =
                        rich_text_runs(input, &display_text, false, &text_style, range.start);
                    (range, display_text, runs)
                })
                .collect()
        };
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let line_height = text_style
            .line_height
            .to_pixels(font_size.into(), window.rem_size());
        let measured = Rc::new(RefCell::new(None));
        let measured_for_layout = measured.clone();
        let layout_id = window.request_measured_layout(
            style,
            move |known_dimensions, available_space, window, _| {
                let wrap_width = known_dimensions.width.or(match available_space.width {
                    AvailableSpace::Definite(width) => Some(width),
                    _ => None,
                });
                let mut lines = Vec::with_capacity(line_specs.len());
                let mut measured_width = Pixels::ZERO;
                let mut measured_height = Pixels::ZERO;
                for (range, display_text, runs) in &line_specs {
                    let line = window
                        .text_system()
                        .shape_text(display_text.clone(), font_size, runs, wrap_width, None)
                        .expect("rich text shaping failed")
                        .pop()
                        .expect("rich text shaping returned no line");
                    let line_size = line.size(line_height);
                    measured_width = measured_width.max(line_size.width);
                    lines.push(RichLayoutLine {
                        range: range.clone(),
                        line,
                        y: measured_height,
                        height: line_size.height,
                    });
                    measured_height += line_size.height;
                }
                let measured_size = size(wrap_width.unwrap_or(measured_width), measured_height);
                measured_for_layout
                    .borrow_mut()
                    .replace(RichMeasuredLayout {
                        layout: RichLayout {
                            lines,
                            height: measured_height,
                        },
                        line_height,
                    });
                measured_size
            },
        );
        (layout_id, measured)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        measured: &mut Self::RequestLayoutState,
        _window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let measured = measured
            .borrow()
            .clone()
            .expect("rich text measurement did not run");
        let layout = measured.layout;
        let line_height = measured.line_height;
        let focused = input.focus_handle.is_focused(_window);
        let selections = if focused && !input.selected_range.is_empty() {
            rich_selection_quads(&layout, input.selected_range.clone(), bounds, line_height)
        } else {
            Vec::new()
        };
        let cursor = (focused && !input.read_only && input.selected_range.is_empty()).then(|| {
            let cursor = layout
                .position_for_index(input.cursor_offset(), line_height)
                .unwrap_or_default();
            fill(
                Bounds::new(
                    point(bounds.left() + cursor.x, bounds.top() + cursor.y + px(2.0)),
                    size(px(1.5), line_height - px(4.0)),
                ),
                rgba((ACCENT << 8) | 0xff),
            )
        });
        RichPrepaintState {
            layout: Some(layout),
            line_height,
            cursor,
            selections,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
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
        let layout = prepaint
            .layout
            .take()
            .expect("rich input layout was shaped");
        for visual_line in &layout.lines {
            let line_bounds = Bounds::new(
                point(bounds.left(), bounds.top() + visual_line.y),
                size(bounds.size.width, visual_line.height),
            );
            visual_line
                .line
                .paint_background(
                    line_bounds.origin,
                    prepaint.line_height,
                    TextAlign::Left,
                    Some(line_bounds),
                    window,
                    cx,
                )
                .expect("rich input background paint failed");
        }
        for selection in prepaint.selections.drain(..) {
            window.paint_quad(selection);
        }
        for visual_line in &layout.lines {
            let line_bounds = Bounds::new(
                point(bounds.left(), bounds.top() + visual_line.y),
                size(bounds.size.width, visual_line.height),
            );
            visual_line
                .line
                .paint(
                    line_bounds.origin,
                    prepaint.line_height,
                    TextAlign::Left,
                    Some(line_bounds),
                    window,
                    cx,
                )
                .expect("rich input line paint failed");
        }
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _| {
            input.last_layout = Some(layout);
            input.last_bounds = Some(bounds);
            input.last_line_height = prepaint.line_height;
        });
    }
}

fn rich_selection_quads(
    layout: &RichLayout,
    selection: Range<usize>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<PaintQuad> {
    let mut quads = Vec::new();
    for visual_line in &layout.lines {
        let start = selection.start.max(visual_line.range.start);
        let end = selection.end.min(visual_line.range.end);
        if start >= end {
            continue;
        }
        let line_bounds = Bounds::new(
            point(bounds.left(), bounds.top() + visual_line.y),
            size(bounds.size.width, visual_line.height),
        );
        quads.extend(selection_quads(
            &visual_line.line,
            start - visual_line.range.start..end - visual_line.range.start,
            line_bounds,
            line_height,
        ));
    }
    quads
}

fn selection_quads(
    line: &WrappedLine,
    selection: Range<usize>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<PaintQuad> {
    let mut line_start = 0;
    let mut line_ends = line
        .wrap_boundaries()
        .iter()
        .map(|boundary| line.runs()[boundary.run_ix].glyphs[boundary.glyph_ix].index)
        .chain([line.len()]);
    let mut quads = Vec::new();
    for (visual_line, line_end) in line_ends.by_ref().enumerate() {
        let start = selection.start.max(line_start);
        let end = selection.end.min(line_end);
        if start < end {
            let start_x = line
                .position_for_index(start, line_height)
                .map_or(Pixels::ZERO, |position| position.x);
            let end_x = line
                .position_for_index(end, line_height)
                .map_or(line.width(), |position| position.x);
            let top = bounds.top() + line_height * visual_line;
            quads.push(fill(
                Bounds::from_corners(
                    point(bounds.left() + start_x, top),
                    point(bounds.left() + end_x, top + line_height),
                ),
                rgba(0x2f6feb2b),
            ));
        }
        line_start = line_end;
    }
    quads
}

fn rich_text_line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, _) in text.match_indices('\n') {
        ranges.push(start..index);
        start = index + 1;
    }
    ranges.push(start..text.len());
    ranges
}

fn rich_text_runs(
    input: &RichInputState,
    display_text: &SharedString,
    placeholder: bool,
    style: &gpui::TextStyle,
    source_start: usize,
) -> Vec<TextRun> {
    if placeholder {
        return vec![TextRun {
            len: display_text.len(),
            font: style.font(),
            color: rgba((INK_FAINT << 8) | 0xff).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }];
    }

    let source_end = source_start + display_text.len();
    let mut boundaries = BTreeSet::from([0, display_text.len()]);
    for span in &input.document.spans {
        let start = span.range.start.max(source_start);
        let end = span.range.end.min(source_end);
        if start < end {
            boundaries.insert(start - source_start);
            boundaries.insert(end - source_start);
        }
    }
    if let Some(marked) = &input.marked_range {
        let start = marked.start.max(source_start);
        let end = marked.end.min(source_end);
        if start < end {
            boundaries.insert(start - source_start);
            boundaries.insert(end - source_start);
        }
    }
    let boundaries = boundaries.into_iter().collect::<Vec<_>>();
    boundaries
        .windows(2)
        .filter_map(|boundary| {
            let range = boundary[0]..boundary[1];
            if range.is_empty() {
                return None;
            }
            let marks = input
                .document
                .spans
                .iter()
                .filter(|span| {
                    span.range.start <= source_start + range.start
                        && span.range.end >= source_start + range.end
                })
                .map(|span| &span.mark)
                .collect::<Vec<_>>();
            let mut run = TextRun {
                len: range.end - range.start,
                font: style.font(),
                color: style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let text_color = marks.iter().find_map(|mark| {
                if let NoteInlineMark::TextColor(color) = mark {
                    Some(*color)
                } else {
                    None
                }
            });
            if let Some(color) = text_color {
                run.color = rgba((color.foreground_rgb() << 8) | 0xff).into();
            }
            if let Some(color) = marks.iter().find_map(|mark| {
                if let NoteInlineMark::Highlight(color) = mark {
                    Some(*color)
                } else {
                    None
                }
            }) {
                run.background_color = Some(rgba((color.background_rgb() << 8) | 0xff).into());
            }
            if marks
                .iter()
                .any(|mark| matches!(mark, NoteInlineMark::Bold))
            {
                run.font = run.font.bold();
            }
            if marks
                .iter()
                .any(|mark| matches!(mark, NoteInlineMark::Italic))
            {
                run.font = run.font.italic();
            }
            if marks
                .iter()
                .any(|mark| matches!(mark, NoteInlineMark::Strikethrough))
            {
                run.strikethrough = Some(StrikethroughStyle {
                    color: Some(run.color),
                    thickness: px(1.0),
                });
            }
            if marks
                .iter()
                .any(|mark| matches!(mark, NoteInlineMark::Code))
                && run.background_color.is_none()
            {
                run.background_color = Some(rgba((ACCENT_SOFT << 8) | 0xff).into());
            }
            if marks
                .iter()
                .any(|mark| matches!(mark, NoteInlineMark::Link(_)))
            {
                if text_color.is_none() {
                    run.color = rgba((ACCENT << 8) | 0xff).into();
                }
                run.underline = Some(UnderlineStyle {
                    color: Some(run.color),
                    thickness: px(1.0),
                    wavy: false,
                });
            }
            if input.marked_range.as_ref().is_some_and(|marked| {
                marked.start <= source_start + range.start && marked.end >= source_start + range.end
            }) {
                run.underline = Some(UnderlineStyle {
                    color: Some(run.color),
                    thickness: px(1.0),
                    wavy: false,
                });
            }
            Some(run)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_line_ranges_preserve_empty_and_trailing_lines() {
        assert_eq!(rich_text_line_ranges(""), vec![0..0]);
        assert_eq!(rich_text_line_ranges("one\ntwo"), vec![0..3, 4..7]);
        assert_eq!(rich_text_line_ranges("\nlast\n"), vec![0..0, 1..5, 6..6]);
    }

    #[test]
    fn rich_clipboard_metadata_has_a_versioned_private_prefix() {
        let metadata = format!("{RICH_CLIPBOARD_METADATA_PREFIX}**bold**");
        assert_eq!(
            metadata.strip_prefix(RICH_CLIPBOARD_METADATA_PREFIX),
            Some("**bold**")
        );
    }
}
