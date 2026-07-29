//! UI-independent inline rich-text model used by native editors.
//!
//! The persisted compatibility projection remains Markdown, while editors work
//! with plain text and byte-range marks so formatting delimiters never need to
//! appear in the active input surface.

use std::{collections::BTreeSet, ops::Range};

use url::Url;

const NOTE_NODE_URL_PREFIX: &str = "ideall://node/";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NoteColor {
    Gray,
    Brown,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    Pink,
    Red,
}

impl NoteColor {
    pub const ALL: [Self; 9] = [
        Self::Gray,
        Self::Brown,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Purple,
        Self::Pink,
        Self::Red,
    ];

    pub const fn foreground_rgb(self) -> u32 {
        match self {
            Self::Gray => 0x787774,
            Self::Brown => 0x9f6b53,
            Self::Orange => 0xd9730d,
            Self::Yellow => 0xcb912f,
            Self::Green => 0x448361,
            Self::Blue => 0x337ea9,
            Self::Purple => 0x9065b0,
            Self::Pink => 0xc14c8a,
            Self::Red => 0xd44c47,
        }
    }

    pub const fn background_rgb(self) -> u32 {
        match self {
            Self::Gray => 0xf1f1ef,
            Self::Brown => 0xf4eeee,
            Self::Orange => 0xfaebdd,
            Self::Yellow => 0xfbf3db,
            Self::Green => 0xedf3ec,
            Self::Blue => 0xe7f3f8,
            Self::Purple => 0xf4f0f7,
            Self::Pink => 0xf9eef3,
            Self::Red => 0xfdebec,
        }
    }

    pub const fn foreground_css(self) -> &'static str {
        match self {
            Self::Gray => "#787774",
            Self::Brown => "#9f6b53",
            Self::Orange => "#d9730d",
            Self::Yellow => "#cb912f",
            Self::Green => "#448361",
            Self::Blue => "#337ea9",
            Self::Purple => "#9065b0",
            Self::Pink => "#c14c8a",
            Self::Red => "#d44c47",
        }
    }

    pub const fn background_css(self) -> &'static str {
        match self {
            Self::Gray => "#f1f1ef",
            Self::Brown => "#f4eeee",
            Self::Orange => "#faebdd",
            Self::Yellow => "#fbf3db",
            Self::Green => "#edf3ec",
            Self::Blue => "#e7f3f8",
            Self::Purple => "#f4f0f7",
            Self::Pink => "#f9eef3",
            Self::Red => "#fdebec",
        }
    }

    pub(crate) fn from_foreground_css(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|color| color.foreground_css().eq_ignore_ascii_case(value))
    }

    pub(crate) fn from_background_css(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|color| color.background_css().eq_ignore_ascii_case(value))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NoteInlineMark {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Link(String),
    TextColor(NoteColor),
    Highlight(NoteColor),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoteInlineSpan {
    pub range: Range<usize>,
    pub mark: NoteInlineMark,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NoteInlineDocument {
    pub text: String,
    pub spans: Vec<NoteInlineSpan>,
}

impl NoteInlineDocument {
    pub fn from_markdown(source: &str) -> Self {
        let mut document = Self::default();
        parse_inline_into(source, &[], &mut document);
        document.normalize();
        document
    }

    pub fn to_markdown(&self) -> String {
        if self.text.is_empty() {
            return String::new();
        }
        let mut boundaries = BTreeSet::from([0, self.text.len()]);
        for span in &self.spans {
            if span.range.start < span.range.end
                && span.range.end <= self.text.len()
                && self.text.is_char_boundary(span.range.start)
                && self.text.is_char_boundary(span.range.end)
            {
                boundaries.insert(span.range.start);
                boundaries.insert(span.range.end);
            }
        }
        let boundaries = boundaries.into_iter().collect::<Vec<_>>();
        let mut rendered = String::new();
        let mut open_marks: Vec<NoteInlineMark> = Vec::new();
        for window in boundaries.windows(2) {
            let range = window[0]..window[1];
            if range.is_empty() {
                continue;
            }
            let marks = ordered_marks(
                self.spans
                    .iter()
                    .filter(|span| span.range.start <= range.start && span.range.end >= range.end)
                    .map(|span| span.mark.clone())
                    .collect(),
            );
            let common = open_marks
                .iter()
                .zip(&marks)
                .take_while(|(open, next)| open == next)
                .count();
            for mark in open_marks[common..].iter().rev() {
                mark.close_into(&mut rendered);
            }
            open_marks.truncate(common);
            for mark in &marks[common..] {
                mark.open_into(&mut rendered);
                open_marks.push(mark.clone());
            }
            push_escaped_text(&mut rendered, &self.text[range]);
        }
        for mark in open_marks.iter().rev() {
            mark.close_into(&mut rendered);
        }
        rendered
    }

    pub fn marks_at(&self, offset: usize) -> BTreeSet<NoteInlineMark> {
        let offset = snap_boundary(&self.text, offset);
        let at_document_end = offset == self.text.len();
        self.spans
            .iter()
            .filter(|span| {
                span.range.start < span.range.end
                    && span.range.start <= offset
                    && (offset < span.range.end || (at_document_end && offset == span.range.end))
            })
            .map(|span| span.mark.clone())
            .collect()
    }

    pub fn slice(&self, range: Range<usize>) -> Self {
        let start = snap_boundary(&self.text, range.start);
        let end = snap_boundary(&self.text, range.end.max(start));
        let mut document = Self {
            text: self.text[start..end].to_owned(),
            spans: self
                .spans
                .iter()
                .filter_map(|span| {
                    let span_start = span.range.start.max(start);
                    let span_end = span.range.end.min(end);
                    (span_start < span_end).then(|| NoteInlineSpan {
                        range: span_start - start..span_end - start,
                        mark: span.mark.clone(),
                    })
                })
                .collect(),
        };
        document.normalize();
        document
    }

    pub fn append(&mut self, other: &Self) {
        let offset = self.text.len();
        self.text.push_str(&other.text);
        self.spans
            .extend(other.spans.iter().cloned().map(|mut span| {
                span.range = span.range.start + offset..span.range.end + offset;
                span
            }));
        self.normalize();
    }

    pub fn replace(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        inherited_marks: &BTreeSet<NoteInlineMark>,
    ) {
        let start = snap_boundary(&self.text, range.start);
        let end = snap_boundary(&self.text, range.end.max(start));
        let removed = end - start;
        self.text.replace_range(start..end, replacement);
        let added = replacement.len();

        let mut adjusted = Vec::new();
        for span in self.spans.drain(..) {
            if span.range.end <= start {
                adjusted.push(span);
                continue;
            }
            if span.range.start >= end {
                adjusted.push(NoteInlineSpan {
                    range: shift_offset(span.range.start, removed, added)
                        ..shift_offset(span.range.end, removed, added),
                    mark: span.mark,
                });
                continue;
            }
            if span.range.start < start {
                adjusted.push(NoteInlineSpan {
                    range: span.range.start..start,
                    mark: span.mark.clone(),
                });
            }
            if span.range.end > end {
                adjusted.push(NoteInlineSpan {
                    range: start + added..shift_offset(span.range.end, removed, added),
                    mark: span.mark,
                });
            }
        }
        if !replacement.is_empty() {
            adjusted.extend(inherited_marks.iter().cloned().map(|mark| NoteInlineSpan {
                range: start..start + added,
                mark,
            }));
        }
        self.spans = adjusted;
        self.normalize();
    }

    pub fn replace_document(&mut self, range: Range<usize>, replacement: &Self) -> usize {
        let start = snap_boundary(&self.text, range.start);
        let end = snap_boundary(&self.text, range.end.max(start));
        let mut updated = self.slice(0..start);
        updated.append(replacement);
        let cursor = updated.text.len();
        updated.append(&self.slice(end..self.text.len()));
        *self = updated;
        cursor
    }

    pub fn set_link(&mut self, range: Range<usize>, url: &str) -> bool {
        let Some(url) = safe_inline_url(url) else {
            return false;
        };
        let start = snap_boundary(&self.text, range.start);
        let end = snap_boundary(&self.text, range.end.max(start));
        if start == end {
            return false;
        }
        self.remove_links(start..end);
        self.spans.push(NoteInlineSpan {
            range: start..end,
            mark: NoteInlineMark::Link(url),
        });
        self.normalize();
        true
    }

    pub fn replace_with_link(
        &mut self,
        range: Range<usize>,
        label: &str,
        url: &str,
    ) -> Option<usize> {
        let url = safe_inline_url(url)?;
        if label.is_empty() {
            return None;
        }
        let replacement = Self {
            text: label.to_owned(),
            spans: vec![NoteInlineSpan {
                range: 0..label.len(),
                mark: NoteInlineMark::Link(url),
            }],
        };
        Some(self.replace_document(range, &replacement))
    }

    pub fn remove_links(&mut self, range: Range<usize>) -> bool {
        let start = snap_boundary(&self.text, range.start);
        let end = snap_boundary(&self.text, range.end.max(start));
        if start == end {
            return false;
        }
        let mut changed = false;
        let mut updated = Vec::new();
        for span in self.spans.drain(..) {
            if !matches!(span.mark, NoteInlineMark::Link(_))
                || span.range.end <= start
                || span.range.start >= end
            {
                updated.push(span);
                continue;
            }
            changed = true;
            if span.range.start < start {
                updated.push(NoteInlineSpan {
                    range: span.range.start..start,
                    mark: span.mark.clone(),
                });
            }
            if span.range.end > end {
                updated.push(NoteInlineSpan {
                    range: end..span.range.end,
                    mark: span.mark,
                });
            }
        }
        self.spans = updated;
        self.normalize();
        changed
    }

    pub fn toggle_mark(&mut self, range: Range<usize>, mark: NoteInlineMark) -> bool {
        let start = snap_boundary(&self.text, range.start);
        let end = snap_boundary(&self.text, range.end.max(start));
        if start == end {
            return false;
        }
        let fully_marked = covered_length(
            self.spans
                .iter()
                .filter(|span| span.mark == mark)
                .map(|span| span.range.clone()),
            start..end,
        ) == end - start;
        if fully_marked {
            let mut updated = Vec::new();
            for span in self.spans.drain(..) {
                if span.mark != mark || span.range.end <= start || span.range.start >= end {
                    updated.push(span);
                    continue;
                }
                if span.range.start < start {
                    updated.push(NoteInlineSpan {
                        range: span.range.start..start,
                        mark: span.mark.clone(),
                    });
                }
                if span.range.end > end {
                    updated.push(NoteInlineSpan {
                        range: end..span.range.end,
                        mark: span.mark,
                    });
                }
            }
            self.spans = updated;
        } else {
            self.spans.push(NoteInlineSpan {
                range: start..end,
                mark,
            });
        }
        self.normalize();
        true
    }

    pub fn set_text_color(&mut self, range: Range<usize>, color: Option<NoteColor>) -> bool {
        self.set_exclusive_color(range, color.map(NoteInlineMark::TextColor), true)
    }

    pub fn set_highlight(&mut self, range: Range<usize>, color: Option<NoteColor>) -> bool {
        self.set_exclusive_color(range, color.map(NoteInlineMark::Highlight), false)
    }

    fn set_exclusive_color(
        &mut self,
        range: Range<usize>,
        mark: Option<NoteInlineMark>,
        foreground: bool,
    ) -> bool {
        let start = snap_boundary(&self.text, range.start);
        let end = snap_boundary(&self.text, range.end.max(start));
        if start == end {
            return false;
        }
        let before = self.spans.clone();
        let mut updated = Vec::new();
        for span in self.spans.drain(..) {
            let same_family = if foreground {
                matches!(span.mark, NoteInlineMark::TextColor(_))
            } else {
                matches!(span.mark, NoteInlineMark::Highlight(_))
            };
            if !same_family || span.range.end <= start || span.range.start >= end {
                updated.push(span);
                continue;
            }
            if span.range.start < start {
                updated.push(NoteInlineSpan {
                    range: span.range.start..start,
                    mark: span.mark.clone(),
                });
            }
            if span.range.end > end {
                updated.push(NoteInlineSpan {
                    range: end..span.range.end,
                    mark: span.mark,
                });
            }
        }
        if let Some(mark) = mark {
            updated.push(NoteInlineSpan {
                range: start..end,
                mark,
            });
        }
        self.spans = updated;
        self.normalize();
        self.spans != before
    }

    fn normalize(&mut self) {
        self.spans.retain(|span| {
            span.range.start < span.range.end
                && span.range.end <= self.text.len()
                && self.text.is_char_boundary(span.range.start)
                && self.text.is_char_boundary(span.range.end)
        });
        self.spans.sort_by(|left, right| {
            left.mark
                .cmp(&right.mark)
                .then(left.range.start.cmp(&right.range.start))
                .then(left.range.end.cmp(&right.range.end))
        });
        let mut normalized: Vec<NoteInlineSpan> = Vec::new();
        for span in self.spans.drain(..) {
            if let Some(previous) = normalized.last_mut()
                && previous.mark == span.mark
                && previous.range.end >= span.range.start
            {
                previous.range.end = previous.range.end.max(span.range.end);
                continue;
            }
            normalized.push(span);
        }
        self.spans = normalized;
    }
}

impl NoteInlineMark {
    fn delimiter(&self) -> &'static str {
        match self {
            Self::Bold => "**",
            Self::Italic => "*",
            Self::Strikethrough => "~~",
            Self::Code => "`",
            Self::Link(_) | Self::TextColor(_) | Self::Highlight(_) => "",
        }
    }

    fn open_into(&self, output: &mut String) {
        match self {
            Self::Link(_) => output.push('['),
            Self::TextColor(color) => output.push_str(color_foreground_open_tag(*color)),
            Self::Highlight(color) => output.push_str(color_highlight_open_tag(*color)),
            _ => output.push_str(self.delimiter()),
        }
    }

    fn close_into(&self, output: &mut String) {
        match self {
            Self::Link(url) => {
                output.push_str("](");
                output.push_str(url);
                output.push(')');
            }
            Self::TextColor(_) => output.push_str("</span>"),
            Self::Highlight(_) => output.push_str("</mark>"),
            _ => output.push_str(self.delimiter()),
        }
    }
}

fn ordered_marks(marks: BTreeSet<NoteInlineMark>) -> Vec<NoteInlineMark> {
    let mut ordered = marks
        .iter()
        .filter(|mark| matches!(mark, NoteInlineMark::Link(_)))
        .cloned()
        .collect::<Vec<_>>();
    ordered.extend(
        marks
            .iter()
            .filter(|mark| matches!(mark, NoteInlineMark::TextColor(_)))
            .cloned(),
    );
    ordered.extend(
        marks
            .iter()
            .filter(|mark| matches!(mark, NoteInlineMark::Highlight(_)))
            .cloned(),
    );
    for mark in [
        NoteInlineMark::Bold,
        NoteInlineMark::Italic,
        NoteInlineMark::Strikethrough,
        NoteInlineMark::Code,
    ] {
        if marks.contains(&mark) {
            ordered.push(mark);
        }
    }
    ordered
}

fn parse_inline_into(source: &str, marks: &[NoteInlineMark], document: &mut NoteInlineDocument) {
    let mut offset = 0;
    while offset < source.len() {
        let remaining = &source[offset..];
        if remaining.starts_with("\\\n") {
            push_text("\n", marks, document);
            offset += 2;
            continue;
        }
        if let Some(escaped) = parse_escape(remaining) {
            push_text(escaped, marks, document);
            offset += 1 + escaped.len();
            continue;
        }
        if let Some((consumed, label, url)) = parse_link(remaining) {
            let mut nested = marks.to_vec();
            nested.push(NoteInlineMark::Link(url.to_owned()));
            parse_inline_into(label, &nested, document);
            offset += consumed;
            continue;
        }
        if let Some((opening_len, closing, mark)) = parse_color_tag(remaining) {
            let inner = &remaining[opening_len..];
            if let Some(end) = find_unescaped(inner, closing) {
                let mut nested = marks.to_vec();
                nested.push(mark);
                parse_inline_into(&inner[..end], &nested, document);
                offset += opening_len + end + closing.len();
                continue;
            }
        }

        let delimiter = if remaining.starts_with("***") {
            Some(("***", vec![NoteInlineMark::Bold, NoteInlineMark::Italic]))
        } else if remaining.starts_with("**") {
            Some(("**", vec![NoteInlineMark::Bold]))
        } else if remaining.starts_with("~~") {
            Some(("~~", vec![NoteInlineMark::Strikethrough]))
        } else if remaining.starts_with('`') {
            Some(("`", vec![NoteInlineMark::Code]))
        } else if remaining.starts_with('*') {
            Some(("*", vec![NoteInlineMark::Italic]))
        } else {
            None
        };
        if let Some((delimiter, added_marks)) = delimiter {
            let inner = &remaining[delimiter.len()..];
            if let Some(end) = find_unescaped(inner, delimiter) {
                let mut nested = marks.to_vec();
                nested.extend(added_marks);
                parse_inline_into(&inner[..end], &nested, document);
                offset += delimiter.len() + end + delimiter.len();
                continue;
            }
        }

        let next = remaining
            .char_indices()
            .skip(1)
            .find(|(_, character)| matches!(character, '\\' | '*' | '~' | '`' | '[' | '<'))
            .map_or(remaining.len(), |(index, _)| index);
        let consumed = next.max(remaining.chars().next().map_or(0, char::len_utf8));
        push_text(&remaining[..consumed], marks, document);
        offset += consumed;
    }
}

fn push_escaped_text(output: &mut String, text: &str) {
    for character in text.chars() {
        if character == '\n' {
            output.push_str("\\\n");
            continue;
        }
        if is_escapable(character) {
            output.push('\\');
        }
        output.push(character);
    }
}

fn parse_escape(value: &str) -> Option<&str> {
    let escaped = value.strip_prefix('\\')?;
    let character = escaped.chars().next()?;
    is_escapable(character).then(|| &escaped[..character.len_utf8()])
}

fn is_escapable(character: char) -> bool {
    matches!(
        character,
        '\\' | '*' | '_' | '~' | '`' | '[' | ']' | '(' | ')' | '<' | '>'
    )
}

pub(crate) fn parse_color_tag(value: &str) -> Option<(usize, &'static str, NoteInlineMark)> {
    for color in NoteColor::ALL {
        let opening = color_foreground_open_tag(color);
        if value.starts_with(opening) {
            return Some((opening.len(), "</span>", NoteInlineMark::TextColor(color)));
        }
        let opening = color_highlight_open_tag(color);
        if value.starts_with(opening) {
            return Some((opening.len(), "</mark>", NoteInlineMark::Highlight(color)));
        }
    }
    None
}

fn color_foreground_open_tag(color: NoteColor) -> &'static str {
    match color {
        NoteColor::Gray => "<span style=\"color:#787774\">",
        NoteColor::Brown => "<span style=\"color:#9f6b53\">",
        NoteColor::Orange => "<span style=\"color:#d9730d\">",
        NoteColor::Yellow => "<span style=\"color:#cb912f\">",
        NoteColor::Green => "<span style=\"color:#448361\">",
        NoteColor::Blue => "<span style=\"color:#337ea9\">",
        NoteColor::Purple => "<span style=\"color:#9065b0\">",
        NoteColor::Pink => "<span style=\"color:#c14c8a\">",
        NoteColor::Red => "<span style=\"color:#d44c47\">",
    }
}

fn color_highlight_open_tag(color: NoteColor) -> &'static str {
    match color {
        NoteColor::Gray => "<mark style=\"background-color:#f1f1ef\">",
        NoteColor::Brown => "<mark style=\"background-color:#f4eeee\">",
        NoteColor::Orange => "<mark style=\"background-color:#faebdd\">",
        NoteColor::Yellow => "<mark style=\"background-color:#fbf3db\">",
        NoteColor::Green => "<mark style=\"background-color:#edf3ec\">",
        NoteColor::Blue => "<mark style=\"background-color:#e7f3f8\">",
        NoteColor::Purple => "<mark style=\"background-color:#f4f0f7\">",
        NoteColor::Pink => "<mark style=\"background-color:#f9eef3\">",
        NoteColor::Red => "<mark style=\"background-color:#fdebec\">",
    }
}

fn find_unescaped(value: &str, needle: &str) -> Option<usize> {
    value.char_indices().find_map(|(index, _)| {
        (value[index..].starts_with(needle) && !is_escaped_at(value, index)).then_some(index)
    })
}

fn is_escaped_at(value: &str, index: usize) -> bool {
    value[..index]
        .chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count()
        % 2
        == 1
}

fn push_text(text: &str, marks: &[NoteInlineMark], document: &mut NoteInlineDocument) {
    let start = document.text.len();
    document.text.push_str(text);
    let end = document.text.len();
    document
        .spans
        .extend(marks.iter().cloned().map(|mark| NoteInlineSpan {
            range: start..end,
            mark,
        }));
}

fn parse_link(value: &str) -> Option<(usize, &str, &str)> {
    let body = value.strip_prefix('[')?;
    let label_end = find_unescaped(body, "](")?;
    let label = &body[..label_end];
    let url_start = 1 + label_end + 2;
    let url_end = find_unescaped(&value[url_start..], ")")? + url_start;
    let url = &value[url_start..url_end];
    safe_inline_url(url).map(|_| (url_end + 1, label, url))
}

fn safe_inline_url(value: &str) -> Option<String> {
    let value = value.trim();
    if note_node_id_from_url(value).is_some() {
        return Some(value.to_owned());
    }
    let parsed = Url::parse(value).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| parsed.to_string())
}

pub fn note_node_url(id: &str) -> Option<String> {
    valid_note_node_id(id).then(|| format!("{NOTE_NODE_URL_PREFIX}{id}"))
}

pub fn note_node_id_from_url(value: &str) -> Option<&str> {
    let id = value.strip_prefix(NOTE_NODE_URL_PREFIX)?;
    valid_note_node_id(id).then_some(id)
}

pub fn note_mention_query(target: &str, cursor: usize) -> Option<(Range<usize>, &str)> {
    let cursor = snap_boundary(target, cursor);
    let line_start = target[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let before_cursor = &target[line_start..cursor];
    let mention_start = before_cursor.rfind('@')?;
    if mention_start > 0 {
        let preceding = before_cursor[..mention_start].chars().next_back()?;
        if !preceding.is_whitespace() && !matches!(preceding, '(' | '[' | '{') {
            return None;
        }
    }
    let query = &before_cursor[mention_start + 1..];
    if query.len() > 96
        || query
            .chars()
            .any(|character| matches!(character, '@' | '[' | ']' | '(' | ')' | '\\' | '/'))
    {
        return None;
    }
    Some((line_start + mention_start..cursor, query.trim_start()))
}

fn valid_note_node_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn shift_offset(offset: usize, removed: usize, added: usize) -> usize {
    offset.saturating_sub(removed).saturating_add(added)
}

fn snap_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn covered_length(ranges: impl Iterator<Item = Range<usize>>, target: Range<usize>) -> usize {
    let mut ranges = ranges
        .filter_map(|range| {
            let start = range.start.max(target.start);
            let end = range.end.min(target.end);
            (start < end).then_some(start..end)
        })
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    let mut total = 0;
    let mut end = target.start;
    for range in ranges {
        if range.start > end {
            break;
        }
        if range.end > end {
            total += range.end - end.max(range.start);
            end = range.end;
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_parses_to_plain_text_and_nested_marks() {
        let document = NoteInlineDocument::from_markdown(
            "***bold and italic*** [safe](https://example.com) `code`",
        );
        assert_eq!(document.text, "bold and italic safe code");
        assert!(document.spans.iter().any(|span| {
            span.mark == NoteInlineMark::Bold
                && &document.text[span.range.clone()] == "bold and italic"
        }));
        assert!(document.spans.iter().any(|span| {
            span.mark == NoteInlineMark::Italic
                && &document.text[span.range.clone()] == "bold and italic"
        }));
        assert_eq!(
            NoteInlineDocument::from_markdown(&document.to_markdown()),
            document
        );
    }

    #[test]
    fn edits_shift_and_inherit_marks_without_exposing_delimiters() {
        let mut document = NoteInlineDocument::from_markdown("hello **world**");
        let world = document.text.find("world").unwrap();
        let inherited = document.marks_at(world + 2);
        document.replace(world + 2..world + 2, "IDE", &inherited);
        assert_eq!(document.text, "hello woIDErld");
        assert_eq!(document.to_markdown(), "hello **woIDErld**");
        document.replace(0..6, "", &BTreeSet::new());
        assert_eq!(document.to_markdown(), "**woIDErld**");
    }

    #[test]
    fn toggling_a_partial_mark_adds_then_removes_the_selected_range() {
        let mut document = NoteInlineDocument::from_markdown("alpha beta");
        assert!(document.toggle_mark(0..5, NoteInlineMark::Bold));
        assert_eq!(document.to_markdown(), "**alpha** beta");
        assert!(document.toggle_mark(0..5, NoteInlineMark::Bold));
        assert_eq!(document.to_markdown(), "alpha beta");
    }

    #[test]
    fn slicing_preserves_only_marks_inside_the_requested_range() {
        let document = NoteInlineDocument::from_markdown("one **two three** four");
        let start = document.text.find("two").unwrap();
        let end = document.text.find(" four").unwrap();
        let sliced = document.slice(start..end);
        assert_eq!(sliced.text, "two three");
        assert_eq!(sliced.to_markdown(), "**two three**");
    }

    #[test]
    fn nested_style_boundaries_serialize_without_repeating_outer_delimiters() {
        let mut document = NoteInlineDocument {
            text: "abc".into(),
            spans: vec![],
        };
        document.toggle_mark(0..3, NoteInlineMark::Bold);
        document.toggle_mark(1..2, NoteInlineMark::Italic);
        assert_eq!(document.to_markdown(), "**a*b*c**");
        assert_eq!(
            NoteInlineDocument::from_markdown(&document.to_markdown()),
            document
        );
    }

    #[test]
    fn appending_documents_offsets_and_merges_compatible_marks() {
        let mut left = NoteInlineDocument::from_markdown("**hello**");
        let right = NoteInlineDocument::from_markdown(" **world**");
        left.append(&right);
        assert_eq!(left.text, "hello world");
        assert_eq!(left.to_markdown(), "**hello** **world**");
    }

    #[test]
    fn unsafe_links_remain_literal_text() {
        let document = NoteInlineDocument::from_markdown("[bad](javascript:alert(1))");
        assert_eq!(document.text, "[bad](javascript:alert(1))");
        assert!(document.spans.is_empty());
    }

    #[test]
    fn literal_markdown_punctuation_round_trips_without_becoming_marks() {
        let document = NoteInlineDocument {
            text: r"\ *literal* _word_ ~~gone~~ `tick` [label](target)".into(),
            spans: Vec::new(),
        };
        let markdown = document.to_markdown();

        assert_eq!(
            markdown,
            r"\\ \*literal\* \_word\_ \~\~gone\~\~ \`tick\` \[label\]\(target\)"
        );
        assert_eq!(NoteInlineDocument::from_markdown(&markdown), document);
    }

    #[test]
    fn escaped_delimiters_inside_marks_do_not_close_the_mark_early() {
        let document = NoteInlineDocument {
            text: "*bold literal* and `code literal`".into(),
            spans: vec![
                NoteInlineSpan {
                    range: 0..14,
                    mark: NoteInlineMark::Bold,
                },
                NoteInlineSpan {
                    range: 19..33,
                    mark: NoteInlineMark::Code,
                },
            ],
        };

        assert_eq!(
            NoteInlineDocument::from_markdown(&document.to_markdown()),
            document
        );
    }

    #[test]
    fn rich_fragments_replace_text_without_losing_marks() {
        let mut document = NoteInlineDocument::from_markdown("before plain after");
        let fragment = NoteInlineDocument::from_markdown("**rich** and `code`");
        let start = document.text.find("plain").unwrap();
        let cursor = document.replace_document(start..start + "plain".len(), &fragment);

        assert_eq!(document.text, "before rich and code after");
        assert_eq!(document.to_markdown(), "before **rich** and `code` after");
        assert_eq!(cursor, "before rich and code".len());
    }

    #[test]
    fn safe_links_can_be_created_updated_and_removed() {
        let mut document = NoteInlineDocument::from_markdown("open site now");
        let range = document.text.find("site").unwrap()..document.text.find(" now").unwrap();

        assert!(document.set_link(range.clone(), " https://example.com/docs "));
        assert_eq!(
            document.to_markdown(),
            "open [site](https://example.com/docs) now"
        );
        assert!(!document.set_link(range.clone(), "javascript:alert(1)"));
        assert!(document.remove_links(range));
        assert_eq!(document.to_markdown(), "open site now");
    }

    #[test]
    fn internal_note_mentions_are_strict_and_round_trip_as_links() {
        let mut document = NoteInlineDocument::from_markdown("See @pro next");
        let range = document.text.find("@pro").unwrap()..document.text.find(" next").unwrap();
        let url = note_node_url("page-123").unwrap();

        let cursor = document
            .replace_with_link(range, "@Project plan", &url)
            .unwrap();

        assert_eq!(cursor, "See @Project plan".len());
        assert_eq!(
            document.to_markdown(),
            "See [@Project plan](ideall://node/page-123) next"
        );
        assert_eq!(
            NoteInlineDocument::from_markdown(&document.to_markdown()),
            document
        );
        assert_eq!(note_node_id_from_url(&url), Some("page-123"));
        assert!(note_node_url("../escape").is_none());
        assert!(note_node_id_from_url("ideall://node/page/child").is_none());
    }

    #[test]
    fn mention_queries_require_a_word_boundary_and_stay_on_one_line() {
        assert_eq!(
            note_mention_query("See @project pl", "See @project pl".len()),
            Some((4.."See @project pl".len(), "project pl"))
        );
        assert_eq!(note_mention_query("mail@example", 12), None);
        assert_eq!(
            note_mention_query("first\n@next", "first\n@next".len()),
            Some((6.."first\n@next".len(), "next"))
        );
        assert_eq!(note_mention_query("@bad/path", 9), None);
    }

    #[test]
    fn soft_breaks_use_a_reversible_single_block_projection() {
        let document = NoteInlineDocument {
            text: "first\nsecond".into(),
            spans: vec![NoteInlineSpan {
                range: 0.."first\nsecond".len(),
                mark: NoteInlineMark::Bold,
            }],
        };

        assert_eq!(document.to_markdown(), "**first\\\nsecond**");
        assert_eq!(
            NoteInlineDocument::from_markdown(&document.to_markdown()),
            document
        );
    }

    #[test]
    fn semantic_text_colors_and_highlights_round_trip_without_visible_editor_tokens() {
        let mut document = NoteInlineDocument::from_markdown("paint this text");
        assert!(document.set_text_color(0..5, Some(NoteColor::Blue)));
        assert!(document.set_highlight(6..10, Some(NoteColor::Yellow)));

        assert_eq!(document.text, "paint this text");
        assert_eq!(
            document.to_markdown(),
            "<span style=\"color:#337ea9\">paint</span> <mark style=\"background-color:#fbf3db\">this</mark> text"
        );
        assert_eq!(
            NoteInlineDocument::from_markdown(&document.to_markdown()),
            document
        );
    }

    #[test]
    fn setting_a_color_replaces_only_the_selected_part_of_the_previous_color() {
        let mut document = NoteInlineDocument::from_markdown("one two three");
        assert!(document.set_text_color(0..13, Some(NoteColor::Red)));
        assert!(document.set_text_color(4..7, Some(NoteColor::Green)));

        assert_eq!(
            document.to_markdown(),
            "<span style=\"color:#d44c47\">one </span><span style=\"color:#448361\">two</span><span style=\"color:#d44c47\"> three</span>"
        );
        assert!(document.set_text_color(4..7, None));
        assert_eq!(
            document.to_markdown(),
            "<span style=\"color:#d44c47\">one </span>two<span style=\"color:#d44c47\"> three</span>"
        );
    }

    #[test]
    fn literal_color_tags_remain_plain_text() {
        let document = NoteInlineDocument {
            text: "<span style=\"color:#337ea9\">plain</span>".into(),
            spans: Vec::new(),
        };

        let markdown = document.to_markdown();
        assert!(markdown.starts_with(r"\<span"));
        assert_eq!(NoteInlineDocument::from_markdown(&markdown), document);
    }
}
