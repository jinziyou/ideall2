use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum NoteEditorBlockKind {
    Paragraph,
    Heading(u8),
    Quote,
    Bullet,
    Ordered,
    Todo,
    Code { language: String },
    Rule,
    Callout { icon: String },
    Toggle { expanded: bool },
    Equation,
    Bookmark,
    Protected,
}

impl NoteEditorBlockKind {
    pub(super) fn placeholder(&self) -> &'static str {
        match self {
            Self::Code { .. } => "输入代码；Enter 换行",
            Self::Callout { .. } => "输入需要强调的内容",
            Self::Toggle { .. } => "第一行作为摘要，其余内容可折叠",
            Self::Equation => "输入 LaTeX 公式",
            Self::Bookmark => "输入完整的 https:// 地址",
            _ => "输入内容，键入 / 选择块类型",
        }
    }

    pub(super) fn supports_inline_rich_text(&self) -> bool {
        matches!(
            self,
            Self::Paragraph
                | Self::Heading(_)
                | Self::Quote
                | Self::Bullet
                | Self::Ordered
                | Self::Todo
                | Self::Callout { .. }
        )
    }
}

pub(super) struct NoteEditorBlock {
    pub(super) id: u64,
    pub(super) kind: NoteEditorBlockKind,
    pub(super) input: Entity<InputState>,
    pub(super) rich_input: Option<Entity<RichInputState>>,
    pub(super) indent: usize,
    pub(super) checked: bool,
}

#[derive(Clone)]
pub(super) struct DraggedNoteBlocks {
    pub(super) ids: Vec<u64>,
}

impl Render for DraggedNoteBlocks {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER_STRONG))
            .bg(rgb(SURFACE_RAISED))
            .text_sm()
            .text_color(rgb(INK))
            .child(if self.ids.len() == 1 {
                "移动 1 个内容块".to_owned()
            } else {
                format!("移动 {} 个内容块", self.ids.len())
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NoteEditorBlockSpec {
    pub(super) kind: NoteEditorBlockKind,
    pub(super) text: String,
    pub(super) indent: usize,
    pub(super) checked: bool,
}

impl NoteEditorBlockSpec {
    pub(super) fn paragraph(text: impl Into<String>) -> Self {
        Self {
            kind: NoteEditorBlockKind::Paragraph,
            text: text.into(),
            indent: 0,
            checked: false,
        }
    }
}

impl NoteEditorBlock {
    pub(super) fn to_spec(&self, cx: &Context<IdeallDesktop>) -> NoteEditorBlockSpec {
        NoteEditorBlockSpec {
            kind: self.kind.clone(),
            text: self
                .rich_input
                .as_ref()
                .map(|input| input.read(cx).markdown())
                .unwrap_or_else(|| self.input.read(cx).value().to_string()),
            indent: self.indent,
            checked: self.checked,
        }
    }

    pub(super) fn markdown(&self, cx: &Context<IdeallDesktop>) -> String {
        render_note_block(&self.to_spec(cx))
    }
}

pub(super) fn parse_note_blocks(text: &str) -> Vec<NoteEditorBlockSpec> {
    let lines = text.split('\n').collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if let Some(icon) = line.strip_prefix(":::callout ") {
            let icon = if icon.trim().is_empty() {
                "💡"
            } else {
                icon.trim()
            };
            let text = parse_directive_body(&lines, &mut index);
            blocks.push(NoteEditorBlockSpec {
                kind: NoteEditorBlockKind::Callout {
                    icon: icon.to_owned(),
                },
                text,
                indent: 0,
                checked: false,
            });
            continue;
        }
        if let Some(state) = line.strip_prefix(":::toggle ") {
            let text = parse_directive_body(&lines, &mut index);
            blocks.push(NoteEditorBlockSpec {
                kind: NoteEditorBlockKind::Toggle {
                    expanded: state.trim() != "closed",
                },
                text,
                indent: 0,
                checked: false,
            });
            continue;
        }
        if line == ":::bookmark" {
            let text = parse_directive_body(&lines, &mut index);
            blocks.push(NoteEditorBlockSpec {
                kind: NoteEditorBlockKind::Bookmark,
                text,
                indent: 0,
                checked: false,
            });
            continue;
        }
        if line == "$$" {
            let mut equation = Vec::new();
            index += 1;
            while index < lines.len() && lines[index] != "$$" {
                equation.push(lines[index]);
                index += 1;
            }
            if index < lines.len() {
                index += 1;
            }
            blocks.push(NoteEditorBlockSpec {
                kind: NoteEditorBlockKind::Equation,
                text: equation.join("\n"),
                indent: 0,
                checked: false,
            });
            continue;
        }
        if let Some(language) = line.strip_prefix("```") {
            let mut code = Vec::new();
            index += 1;
            while index < lines.len() && lines[index] != "```" {
                code.push(lines[index]);
                index += 1;
            }
            if index < lines.len() {
                index += 1;
            }
            blocks.push(NoteEditorBlockSpec {
                kind: NoteEditorBlockKind::Code {
                    language: language.to_owned(),
                },
                text: code.join("\n"),
                indent: 0,
                checked: false,
            });
            continue;
        }
        if line.starts_with("⟦ideall:受保护块:") {
            blocks.push(NoteEditorBlockSpec {
                kind: NoteEditorBlockKind::Protected,
                text: line.to_owned(),
                indent: 0,
                checked: false,
            });
            index += 1;
            continue;
        }
        if line.starts_with("> ") {
            let mut quote = Vec::new();
            while index < lines.len() {
                let Some(value) = lines[index].strip_prefix("> ") else {
                    break;
                };
                quote.push(value);
                index += 1;
            }
            blocks.push(NoteEditorBlockSpec {
                kind: NoteEditorBlockKind::Quote,
                text: quote.join("\n"),
                indent: 0,
                checked: false,
            });
            continue;
        }

        let mut logical_line = line.to_owned();
        index += 1;
        while has_markdown_hard_break(&logical_line) && index < lines.len() {
            logical_line.push('\n');
            logical_line.push_str(lines[index]);
            index += 1;
        }
        blocks.push(parse_note_line(&logical_line));
    }
    if blocks.is_empty() {
        blocks.push(NoteEditorBlockSpec::paragraph(""));
    }
    blocks
}

fn has_markdown_hard_break(line: &str) -> bool {
    line.chars()
        .rev()
        .take_while(|character| *character == '\\')
        .count()
        % 2
        == 1
}

fn parse_directive_body(lines: &[&str], index: &mut usize) -> String {
    *index += 1;
    let mut body = Vec::new();
    while *index < lines.len() && lines[*index] != ":::" {
        body.push(lines[*index]);
        *index += 1;
    }
    if *index < lines.len() {
        *index += 1;
    }
    body.join("\n")
}

fn parse_note_line(line: &str) -> NoteEditorBlockSpec {
    if line == "---" {
        return NoteEditorBlockSpec {
            kind: NoteEditorBlockKind::Rule,
            text: String::new(),
            indent: 0,
            checked: false,
        };
    }
    let heading = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if (1..=6).contains(&heading) && line.as_bytes().get(heading) == Some(&b' ') {
        return NoteEditorBlockSpec {
            kind: NoteEditorBlockKind::Heading(heading as u8),
            text: line[heading + 1..].to_owned(),
            indent: 0,
            checked: false,
        };
    }

    let leading_spaces = line.len() - line.trim_start_matches(' ').len();
    let indent = leading_spaces / 2;
    let value = &line[leading_spaces..];
    for (prefix, checked) in [("- [ ] ", false), ("- [x] ", true), ("- [X] ", true)] {
        if let Some(text) = value.strip_prefix(prefix) {
            return NoteEditorBlockSpec {
                kind: NoteEditorBlockKind::Todo,
                text: text.to_owned(),
                indent,
                checked,
            };
        }
    }
    if let Some(text) = value
        .strip_prefix("- ")
        .or_else(|| value.strip_prefix("* "))
    {
        return NoteEditorBlockSpec {
            kind: NoteEditorBlockKind::Bullet,
            text: text.to_owned(),
            indent,
            checked: false,
        };
    }
    if let Some((number, text)) = value.split_once(". ")
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
    {
        return NoteEditorBlockSpec {
            kind: NoteEditorBlockKind::Ordered,
            text: text.to_owned(),
            indent,
            checked: false,
        };
    }
    NoteEditorBlockSpec::paragraph(line)
}

pub(super) fn render_note_block(block: &NoteEditorBlockSpec) -> String {
    let indentation = "  ".repeat(block.indent);
    match &block.kind {
        NoteEditorBlockKind::Paragraph => block.text.clone(),
        NoteEditorBlockKind::Heading(level) => {
            format!(
                "{} {}",
                "#".repeat(usize::from(*level).clamp(1, 6)),
                block.text
            )
        }
        NoteEditorBlockKind::Quote => block
            .text
            .split('\n')
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        NoteEditorBlockKind::Bullet => format!("{indentation}- {}", block.text),
        NoteEditorBlockKind::Ordered => format!("{indentation}1. {}", block.text),
        NoteEditorBlockKind::Todo => format!(
            "{indentation}- [{}] {}",
            if block.checked { "x" } else { " " },
            block.text
        ),
        NoteEditorBlockKind::Code { language } => {
            format!("```{language}\n{}\n```", block.text)
        }
        NoteEditorBlockKind::Rule => "---".into(),
        NoteEditorBlockKind::Callout { icon } => {
            format!(":::callout {icon}\n{}\n:::", block.text)
        }
        NoteEditorBlockKind::Toggle { expanded } => format!(
            ":::toggle {}\n{}\n:::",
            if *expanded { "open" } else { "closed" },
            block.text
        ),
        NoteEditorBlockKind::Equation => format!("$$\n{}\n$$", block.text),
        NoteEditorBlockKind::Bookmark => format!(":::bookmark\n{}\n:::", block.text),
        NoteEditorBlockKind::Protected => block.text.clone(),
    }
}

pub(super) fn note_blocks_markdown(
    blocks: &[NoteEditorBlock],
    cx: &Context<IdeallDesktop>,
) -> String {
    blocks
        .iter()
        .map(|block| block.markdown(cx))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn note_blocks_plain_text(
    blocks: &[NoteEditorBlock],
    cx: &Context<IdeallDesktop>,
) -> String {
    blocks
        .iter()
        .map(|block| {
            block
                .rich_input
                .as_ref()
                .map(|input| input.read(cx).text().to_owned())
                .unwrap_or_else(|| block.input.read(cx).value().to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_blocks_parse_and_render_supported_markdown_without_loss() {
        let source = "# Title\nparagraph\n  - nested\n- [x] done\n> one\n> two\n```rust\nfn main() {}\n```\n---";
        let blocks = parse_note_blocks(source);

        assert_eq!(blocks.len(), 7);
        assert_eq!(blocks[0].kind, NoteEditorBlockKind::Heading(1));
        assert_eq!(blocks[2].kind, NoteEditorBlockKind::Bullet);
        assert_eq!(blocks[2].indent, 1);
        assert_eq!(blocks[3].kind, NoteEditorBlockKind::Todo);
        assert!(blocks[3].checked);
        assert_eq!(blocks[4].text, "one\ntwo");
        assert_eq!(
            blocks
                .iter()
                .map(render_note_block)
                .collect::<Vec<_>>()
                .join("\n"),
            source
        );
    }

    #[test]
    fn empty_documents_keep_one_editable_paragraph() {
        assert_eq!(
            parse_note_blocks(""),
            vec![NoteEditorBlockSpec::paragraph("")]
        );
    }

    #[test]
    fn advanced_blocks_parse_and_render_without_losing_state() {
        let source = ":::callout ✅\nDone\n:::\n:::toggle closed\nDetails\n:::\n$$\nx + y\n$$\n:::bookmark\nhttps://example.com\n:::";
        let blocks = parse_note_blocks(source);

        assert_eq!(blocks.len(), 4);
        assert_eq!(
            blocks[0].kind,
            NoteEditorBlockKind::Callout { icon: "✅".into() }
        );
        assert_eq!(
            blocks[1].kind,
            NoteEditorBlockKind::Toggle { expanded: false }
        );
        assert_eq!(blocks[2].kind, NoteEditorBlockKind::Equation);
        assert_eq!(blocks[3].kind, NoteEditorBlockKind::Bookmark);
        assert_eq!(
            blocks
                .iter()
                .map(render_note_block)
                .collect::<Vec<_>>()
                .join("\n"),
            source
        );
    }

    #[test]
    fn markdown_hard_breaks_stay_inside_one_rich_block() {
        let source = "first\\\n**second**\nafter";
        let blocks = parse_note_blocks(source);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, NoteEditorBlockKind::Paragraph);
        assert_eq!(blocks[0].text, "first\\\n**second**");
        assert_eq!(
            blocks
                .iter()
                .map(render_note_block)
                .collect::<Vec<_>>()
                .join("\n"),
            source
        );

        let literal_backslash = parse_note_blocks("literal\\\\\nafter");
        assert_eq!(literal_backslash.len(), 2);
    }

    #[test]
    fn prose_like_blocks_share_the_native_inline_rich_text_surface() {
        assert!(NoteEditorBlockKind::Paragraph.supports_inline_rich_text());
        assert!(NoteEditorBlockKind::Quote.supports_inline_rich_text());
        assert!(
            NoteEditorBlockKind::Callout {
                icon: "💡".into()
            }
            .supports_inline_rich_text()
        );
        assert!(!NoteEditorBlockKind::Toggle { expanded: true }.supports_inline_rich_text());
        assert!(
            !NoteEditorBlockKind::Code {
                language: "rust".into()
            }
            .supports_inline_rich_text()
        );
    }
}
