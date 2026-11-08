//! Parse `.flow.md` documents.
//!
//! Layout: optional YAML frontmatter, `# Title`, paragraphs (the "why"),
//! then `## Given / When / Then / Cleanup` sections whose list items are
//! steps. The bold text of an item is its intent; fenced blocks with the
//! info string `http`, `browser`, `expect` or `use` beneath it are the
//! machine part.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::design::{Design, DesignKind, parse_chart, parse_screen, parse_storyboard};
use crate::model::{BlockKind, Column, Flow, FlowMeta, NoteSection, RawBlock, Section, SectionKind, Stage, Step, Table, TableRow, column_key, slug};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Diagnostic {
    pub line: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "line {}: {sev}: {}", self.line, self.message)
    }
}

pub struct Parsed {
    pub flow: Flow,
    pub diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
}

/// Parse YAML (with forgiving handling of unquoted `{{templates}}`) into JSON.
pub fn parse_yaml(text: &str) -> Result<Value, String> {
    let fixed = quote_templates(text);
    if fixed.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_norway::from_str::<Value>(&fixed).map_err(|e| e.to_string())
}

/// Wrap unquoted scalars containing `{{…}}` in double quotes, so
/// `lead_id: {{lead_id}}` means the string `"{{lead_id}}"` rather than a
/// YAML flow mapping.
pub fn quote_templates(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    // Inside a `|` / `>` block scalar: lines indented deeper than this are literal.
    let mut literal_indent: Option<usize> = None;
    let indent_of = |l: &str| l.len() - l.trim_start().len();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if let Some(ind) = literal_indent {
            if line.trim().is_empty() || indent_of(line) > ind {
                out.push_str(line);
                continue;
            }
            literal_indent = None;
        }
        let t = line.trim_end();
        let starts_block = ["|", ">", "|-", ">-", "|+", ">+"].iter().any(|s| t.ends_with(&format!(": {s}")) || t.ends_with(&format!("- {s}")));
        if starts_block {
            literal_indent = Some(indent_of(line));
        }
        if !line.contains("{{") {
            out.push_str(line);
            continue;
        }
        out.push_str(&quote_line(line));
    }
    out
}

fn quote_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut quote: Option<char> = None;
    // Start of the current plain scalar (index into chars), if any.
    let mut scalar_start: Option<usize> = None;
    let is_delim = |c: char| matches!(c, '{' | '}' | '[' | ']' | ',');
    let flush = |out: &mut String, s: &[char]| {
        let text: String = s.iter().collect();
        let trimmed_end = text.trim_end();
        let trailing = &text[trimmed_end.len()..];
        if trimmed_end.contains("{{") {
            out.push('"');
            for ch in trimmed_end.chars() {
                if ch == '"' || ch == '\\' {
                    out.push('\\');
                }
                out.push(ch);
            }
            out.push('"');
        } else {
            out.push_str(trimmed_end);
        }
        out.push_str(trailing);
    };
    let mut flow_depth = 0i32;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            out.push(c);
            if c == q {
                quote = None;
            } else if c == '\\' && q == '"' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            }
            i += 1;
            continue;
        }
        if let Some(start) = scalar_start {
            // A scalar ends at a flow delimiter (inside flow collections), a comment, or `: `.
            let at_template = c == '}' && i + 1 < chars.len() && chars[i + 1] == '}';
            let ends = (flow_depth > 0 && is_delim(c) && !at_template && !(c == '{' && chars.get(i + 1) == Some(&'{')))
                || (c == '#' && i > 0 && chars[i - 1] == ' ')
                || (c == ':' && chars.get(i + 1).is_none_or(|n| *n == ' ') && !inside_template(&chars[start..i]));
            if at_template {
                i += 2;
                continue;
            }
            if ends {
                flush(&mut out, &chars[start..i]);
                scalar_start = None;
                continue; // re-handle this char outside a scalar
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                out.push(c);
            }
            '{' if chars.get(i + 1) == Some(&'{') => {
                scalar_start = Some(i);
                i += 2;
                continue;
            }
            '{' | '[' => {
                flow_depth += 1;
                out.push(c);
            }
            '}' | ']' => {
                flow_depth -= 1;
                out.push(c);
            }
            '#' if i == 0 || chars[i - 1] == ' ' => {
                out.extend(&chars[i..]);
                return out;
            }
            ' ' | ',' | ':' | '-' | '\t' => out.push(c),
            '|' | '>' => out.push(c),
            _ => {
                // Start of a plain scalar (key or value).
                scalar_start = Some(i);
                i += 1;
                continue;
            }
        }
        i += 1;
    }
    if let Some(start) = scalar_start {
        flush(&mut out, &chars[start..]);
    }
    out
}

fn inside_template(s: &[char]) -> bool {
    let text: String = s.iter().collect();
    text.matches("{{").count() > text.matches("}}").count()
}

fn sha256(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Derive an id from a path: `flows/quotes/accept-and-pay.flow.md` → `quotes.accept-and-pay`.
pub fn id_from_path(path: &str) -> String {
    let p = path.replace('\\', "/");
    let p = p.strip_suffix(".flow.md").or_else(|| p.strip_suffix(".md")).unwrap_or(&p);
    let p = p.strip_prefix("flows/").or_else(|| p.strip_prefix("fragments/")).unwrap_or(p);
    p.replace('/', ".")
}

fn split_frontmatter(source: &str) -> (Option<(&str, usize)>, &str, usize) {
    let s = source.strip_prefix('\u{feff}').unwrap_or(source);
    if !(s.starts_with("---\n") || s.starts_with("---\r\n")) {
        return (None, s, 0);
    }
    let after = &s[s.find('\n').unwrap() + 1..];
    let mut offset = 0;
    for line in after.split_inclusive('\n') {
        if line.trim_end() == "---" {
            let fm = &after[..offset];
            let body = &after[offset + line.len()..];
            let body_line = 1 + fm.lines().count() + 1; // lines before body
            return (Some((fm, 2)), body, body_line);
        }
        offset += line.len();
    }
    (None, s, 0)
}

fn meta_from(value: &Value, diags: &mut Vec<Diagnostic>) -> (Option<String>, FlowMeta) {
    let mut meta = FlowMeta::default();
    let mut id = None;
    let Some(map) = value.as_object() else {
        if !value.is_null() {
            diags.push(Diagnostic { line: 1, severity: Severity::Error, message: "frontmatter must be a YAML mapping".into() });
        }
        return (id, meta);
    };
    let as_list = |v: &Value| -> Vec<String> {
        match v {
            Value::Array(a) => a.iter().map(crate::results::value_preview).collect(),
            Value::String(s) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
            Value::Null => vec![],
            other => vec![crate::results::value_preview(other)],
        }
    };
    for (k, v) in map {
        match k.as_str() {
            "id" => id = v.as_str().map(String::from),
            "stage" | "status" => match v.as_str().and_then(Stage::parse) {
                Some(st) => meta.stage = st,
                None => diags.push(Diagnostic { line: 1, severity: Severity::Error, message: format!("unknown stage {v}; use draft, review, approved or implemented") }),
            },
            "approved_by" => meta.approved_by = Some(crate::results::value_preview(v)),
            "area" => meta.area = Some(crate::results::value_preview(v)),
            "tags" => meta.tags = as_list(v),
            "owner" => meta.owner = Some(crate::results::value_preview(v)),
            "priority" => meta.priority = Some(crate::results::value_preview(v)),
            "ticket" => meta.ticket = Some(crate::results::value_preview(v)),
            "env" | "envs" => meta.envs = as_list(v),
            "requires" => meta.requires = as_list(v),
            "skip" => meta.skip = v.as_bool().unwrap_or(false),
            _ => {
                meta.extra.insert(k.clone(), v.clone());
            }
        }
    }
    (id, meta)
}

/// Parse a flow document. `path` is relative to the project root.
pub fn parse_flow(path: &str, source: &str) -> Parsed {
    let mut diags = Vec::new();
    let (fm, body, body_line0) = split_frontmatter(source);
    let (fm_id, meta) = match fm {
        Some((text, _)) => match parse_yaml(text) {
            Ok(v) => meta_from(&v, &mut diags),
            Err(e) => {
                diags.push(Diagnostic { line: 2, severity: Severity::Error, message: format!("frontmatter is not valid YAML: {e}") });
                (None, FlowMeta::default())
            }
        },
        None => (None, FlowMeta::default()),
    };
    let stage = meta.stage;
    let line_of = |offset: usize| -> usize { body_line0 + body[..offset.min(body.len())].matches('\n').count() + 1 };

    let mut title = String::new();
    let mut why_parts: Vec<String> = Vec::new();
    let mut sections: Vec<Section> = Vec::new();
    // None = before any H2; Some(None) = an H2 that is not a step section.
    let mut current: Option<Option<SectionKind>> = None;

    let mut heading: Option<(HeadingLevel, String, usize)> = None;
    let mut heading_start = 0usize;
    // A `##` section that is not Given/When/Then/Cleanup: (heading, content start).
    let mut note_open: Option<(String, usize)> = None;
    let mut notes: Vec<NoteSection> = Vec::new();
    let mut questions: Vec<String> = Vec::new();
    let close_note = |open: &mut Option<(String, usize)>, end: usize, notes: &mut Vec<NoteSection>, questions: &mut Vec<String>| {
        if let Some((h, start)) = open.take() {
            let md = body[start..end.max(start)].trim().to_string();
            let lower = h.to_ascii_lowercase();
            if lower == "open questions" || lower == "questions" {
                for line in md.lines() {
                    let t = line.trim_start();
                    let item = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.split_once(". ").filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit())).map(|(_, r)| r));
                    match item {
                        Some(i) => questions.push(i.trim().to_string()),
                        None if !t.is_empty() => {
                            if let Some(last) = questions.last_mut() {
                                last.push(' ');
                                last.push_str(t.trim());
                            }
                        }
                        None => {}
                    }
                }
            } else if !md.is_empty() {
                notes.push(NoteSection { heading: h, markdown: md });
            }
        }
    };
    let mut list_depth = 0usize;
    let mut code: Option<(String, String, usize)> = None; // (info, text, line)
    let mut in_strong = 0usize;
    let mut para_start: Option<usize> = None;

    struct Pending {
        line: usize,
        text: String,
        strong: String,
        intent_done: bool,
        notes: String,
        blocks: Vec<RawBlock>,
        designs: Vec<Design>,
        tables: Vec<Table>,
    }
    /// A table being read: its header, rows, and the cell in progress.
    struct TableBuild {
        name: Option<String>,
        line: usize,
        header: Vec<String>,
        rows: Vec<TableRow>,
        row: Vec<String>,
        row_line: usize,
        cell: String,
    }
    let mut table: Option<TableBuild> = None;
    let mut pending: Option<Pending> = None;
    let mut flow_designs: Vec<Design> = Vec::new();
    let mut used_keys: std::collections::HashMap<String, usize> = Default::default();
    let mut number = 0u32;

    let mut finish_step = |p: Pending, kind: SectionKind, sections: &mut Vec<Section>, diags: &mut Vec<Diagnostic>| {
        let mut intent = if p.strong.trim().is_empty() { p.text.trim().to_string() } else { p.strong.trim().to_string() };
        intent = intent.trim_end_matches(':').trim().to_string();
        if intent.is_empty() {
            diags.push(Diagnostic { line: p.line, severity: Severity::Error, message: "step has no sentence; start the list item with a **bold sentence**".into() });
            return;
        }
        if p.blocks.is_empty() && kind != SectionKind::Cleanup && stage == Stage::Implemented {
            diags.push(Diagnostic {
                line: p.line,
                severity: Severity::Warning,
                message: format!("step \"{intent}\" has no http, browser, expect, use, exec or email block yet, so it will show as pending"),
            });
        }
        let base = slug(&intent);
        let n = used_keys.entry(base.clone()).or_insert(0);
        *n += 1;
        let key = if *n == 1 { base } else { format!("{base}-{n}") };
        let num = if kind == SectionKind::Cleanup {
            None
        } else {
            number += 1;
            Some(number)
        };
        let tables = check_tables(p.tables, &p.blocks, diags);
        let step = Step { key, number: num, intent, notes: p.notes.trim().to_string(), blocks: p.blocks, designs: p.designs, tables, line: p.line };
        match sections.last_mut() {
            Some(s) if s.kind == kind => s.steps.push(step),
            _ => sections.push(Section { kind, steps: vec![step] }),
        }
    };

    let section_kind = |cur: &Option<Option<SectionKind>>| -> Option<SectionKind> { cur.and_then(|k| k) };

    for (event, range) in Parser::new_ext(body, Options::ENABLE_TABLES).into_offset_iter() {
        // Tables of examples under a step (tables elsewhere are only documentation).
        if table.is_some() || matches!(event, Event::Start(Tag::Table(_))) {
            match &event {
                Event::Start(Tag::Table(_)) => {
                    let mut name = None;
                    if let Some(p) = pending.as_mut() {
                        // A `Table: orders` line just above names it.
                        let t = p.notes.trim_end();
                        if let Some(i) = t.rfind("Table:") {
                            let rest = t[i + "Table:".len()..].trim();
                            if !rest.is_empty() && !rest.contains(char::is_whitespace) && (i == 0 || t[..i].ends_with(' ')) {
                                name = Some(rest.to_string());
                                p.notes.truncate(i);
                            }
                        }
                        p.intent_done = true;
                    }
                    table = Some(TableBuild { name, line: line_of(range.start), header: vec![], rows: vec![], row: vec![], row_line: 0, cell: String::new() });
                }
                Event::Start(Tag::TableRow) | Event::Start(Tag::TableHead) => {
                    if let Some(t) = table.as_mut() {
                        t.row.clear();
                        t.row_line = line_of(range.start);
                    }
                }
                Event::Start(Tag::TableCell) => {
                    if let Some(t) = table.as_mut() {
                        t.cell.clear();
                    }
                }
                Event::Text(x) | Event::Code(x) => {
                    if let Some(t) = table.as_mut() {
                        t.cell.push_str(x);
                    }
                }
                Event::End(TagEnd::TableCell) => {
                    if let Some(t) = table.as_mut() {
                        let c = std::mem::take(&mut t.cell);
                        t.row.push(c.trim().to_string());
                    }
                }
                Event::End(TagEnd::TableHead) => {
                    if let Some(t) = table.as_mut() {
                        t.header = std::mem::take(&mut t.row);
                    }
                }
                Event::End(TagEnd::TableRow) => {
                    if let Some(t) = table.as_mut() {
                        let cells = std::mem::take(&mut t.row);
                        t.rows.push(TableRow { cells, line: t.row_line });
                    }
                }
                Event::End(TagEnd::Table) => {
                    if let Some(t) = table.take()
                        && let Some(p) = pending.as_mut()
                    {
                        let columns = t.header.iter().map(|h| Column::new(h)).collect();
                        p.tables.push(Table { name: t.name, columns, rows: t.rows, link: None, line: t.line });
                    }
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                if level <= HeadingLevel::H2 {
                    close_note(&mut note_open, range.start, &mut notes, &mut questions);
                }
                heading_start = range.end;
                heading = Some((level, String::new(), line_of(range.start)));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, text, line)) = heading.take() {
                    let text = text.trim().to_string();
                    if level == HeadingLevel::H1 && title.is_empty() {
                        title = text;
                    } else if level == HeadingLevel::H2 {
                        let kind = SectionKind::parse(&text);
                        if kind.is_none() {
                            let _ = line;
                            note_open = Some((text, heading_start));
                        }
                        current = Some(kind);
                    }
                }
            }
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::Item) if list_depth == 1 && section_kind(&current).is_some() => {
                pending = Some(Pending { line: line_of(range.start), text: String::new(), strong: String::new(), intent_done: false, notes: String::new(), blocks: vec![], designs: vec![], tables: vec![] });
            }
            Event::End(TagEnd::Item) if list_depth == 1 => {
                if let (Some(p), Some(kind)) = (pending.take(), section_kind(&current)) {
                    finish_step(p, kind, &mut sections, &mut diags);
                }
            }
            Event::Start(Tag::Paragraph) => {
                if current.is_none() && heading.is_none() && pending.is_none() && list_depth == 0 {
                    para_start = Some(range.start);
                }
                if let Some(p) = pending.as_mut()
                    && (!p.text.is_empty() || !p.strong.is_empty())
                {
                    p.intent_done = true;
                }
            }
            Event::End(TagEnd::Paragraph) => {
                if let Some(start) = para_start.take()
                    && !title.is_empty()
                {
                    why_parts.push(body[start..range.end].trim().to_string());
                }
                if let Some(p) = pending.as_mut()
                    && (!p.text.is_empty() || !p.strong.is_empty())
                {
                    p.intent_done = true;
                }
            }
            Event::Start(Tag::Strong) => in_strong += 1,
            Event::End(TagEnd::Strong) => in_strong = in_strong.saturating_sub(1),
            Event::Start(Tag::CodeBlock(kind)) => {
                let info = match kind {
                    CodeBlockKind::Fenced(info) => info.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                if let Some(p) = pending.as_mut() {
                    p.intent_done = true;
                }
                code = Some((info, String::new(), line_of(range.start)));
            }
            Event::End(TagEnd::CodeBlock) => {
                let Some((info, text, line)) = code.take() else { continue };
                if let Some(dk) = DesignKind::parse(&info) {
                    let design = parse_design(dk, &text, line, &mut diags);
                    match pending.as_mut() {
                        Some(p) => p.designs.push(design),
                        None => flow_designs.push(design),
                    }
                    continue;
                }
                let Some(kind) = BlockKind::parse(&info) else { continue };
                let value = match parse_yaml(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        diags.push(Diagnostic { line, severity: Severity::Error, message: format!("{} block is not valid YAML: {e}", kind.as_str()) });
                        Value::Null
                    }
                };
                let block = RawBlock { kind, source: text.trim_end().to_string(), value, line };
                if let Some(p) = pending.as_mut() {
                    p.blocks.push(block);
                } else if let (Some(kind_s), Some(last)) = (section_kind(&current), sections.last_mut()) {
                    if last.kind == kind_s
                        && let Some(step) = last.steps.last_mut()
                    {
                        step.blocks.push(block);
                    }
                } else if section_kind(&current).is_some() {
                    diags.push(Diagnostic { line, severity: Severity::Warning, message: "block is not under a step; put it inside a numbered step".into() });
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, text, _)) = code.as_mut() {
                    text.push_str(&t);
                } else if let Some((_, text, _)) = heading.as_mut() {
                    text.push_str(&t);
                } else if let Some(p) = pending.as_mut() {
                    if list_depth > 1 || p.intent_done {
                        p.notes.push_str(&t);
                        p.notes.push(' ');
                    } else {
                        if in_strong > 0 {
                            p.strong.push_str(&t);
                        }
                        p.text.push_str(&t);
                    }
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(p) = pending.as_mut() {
                    if p.intent_done || list_depth > 1 {
                        p.notes.push(' ');
                    } else {
                        p.text.push(' ');
                        if in_strong > 0 {
                            p.strong.push(' ');
                        }
                    }
                }
            }
            _ => {}
        }
    }

    close_note(&mut note_open, body.len(), &mut notes, &mut questions);
    if title.is_empty() {
        diags.push(Diagnostic { line: body_line0 + 1, severity: Severity::Error, message: "missing `# Title` (the business outcome this flow proves)".into() });
    }
    if !stage.runs() && !questions.is_empty() {
        // fine: drafts are where questions live
    } else if stage == Stage::Implemented && !questions.is_empty() {
        diags.push(Diagnostic { line: body_line0 + 1, severity: Severity::Warning, message: format!("flow is implemented but still has {} open question(s)", questions.len()) });
    }
    if sections.iter().all(|s| s.steps.is_empty()) {
        diags.push(Diagnostic { line: body_line0 + 1, severity: Severity::Error, message: "no steps: add `## Given/When/Then` sections with numbered **bold sentence** items".into() });
    }
    if why_parts.is_empty() && !title.is_empty() {
        diags.push(Diagnostic { line: body_line0 + 1, severity: Severity::Warning, message: "no paragraph under the title explaining why this flow matters".into() });
    }

    // Typed validation of each block (templates unresolved).
    for step in sections.iter().flat_map(|s| s.steps.iter()) {
        for b in &step.blocks {
            if b.value.is_null() {
                continue;
            }
            if let Err(e) = crate::blocks::Block::from_raw(b.kind, &b.value) {
                diags.push(Diagnostic { line: b.line, severity: Severity::Error, message: format!("{} block: {e}", b.kind.as_str()) });
            }
        }
    }

    // Designs: tie chart boxes to steps, and check storyboards.
    let numbered: Vec<(u32, String)> = sections.iter().flat_map(|s| s.steps.iter()).filter_map(|st| st.number.map(|n| (n, st.key.clone()))).collect();
    let mut screen_names: Vec<String> =
        sections.iter().flat_map(|s| s.steps.iter()).flat_map(|st| st.designs.iter()).chain(flow_designs.iter()).filter_map(|d| d.screen.as_ref().and_then(|s| s.name.clone())).collect();
    screen_names.sort();
    let mut resolve = |d: &mut Design, diags: &mut Vec<Diagnostic>| {
        if let Some(c) = d.chart.as_mut() {
            for n in c.nodes.iter_mut() {
                if let Some(num) = n.number {
                    match numbered.iter().find(|(k, _)| *k == num) {
                        Some((_, key)) => n.step = Some(key.clone()),
                        None => diags.push(Diagnostic {
                            line: d.line,
                            severity: Severity::Error,
                            message: format!("flowchart box `{}` refers to step {num}, but this flow has {}", n.id, plural(numbered.len(), "step")),
                        }),
                    }
                }
            }
        }
        for t in &d.transitions {
            if let Some(num) = t.step
                && !numbered.iter().any(|(k, _)| *k == num)
            {
                diags.push(Diagnostic { line: d.line, severity: Severity::Error, message: format!("storyboard refers to step {num}, but this flow has {}", plural(numbered.len(), "step")) });
            }
            for name in [&t.from, &t.to] {
                if !screen_names.iter().any(|s| s == name) {
                    screen_names.push(name.clone()); // checked against screens/ when the project loads
                }
            }
        }
    };
    for sec in sections.iter_mut() {
        for st in sec.steps.iter_mut() {
            for d in st.designs.iter_mut() {
                resolve(d, &mut diags);
            }
        }
    }
    for d in flow_designs.iter_mut() {
        resolve(d, &mut diags);
    }
    let mut meta = meta;
    let sample = match meta.extra.remove("sample") {
        Some(Value::Object(m)) => m,
        Some(_) => {
            diags.push(Diagnostic { line: 1, severity: Severity::Error, message: "`sample` must be a mapping of example values, e.g. `sample: { quote_id: q_1 }`".into() });
            Default::default()
        }
        None => Default::default(),
    };

    let flow = Flow {
        id: fm_id.unwrap_or_else(|| id_from_path(path)),
        title,
        why: why_parts.join("\n\n"),
        meta,
        sections,
        questions,
        notes,
        designs: flow_designs,
        sample,
        path: path.replace('\\', "/"),
        source_hash: sha256(source),
        source: source.to_string(),
    };
    Parsed { flow, diagnostics: diags }
}

fn parse_design(kind: DesignKind, text: &str, line: usize, diags: &mut Vec<Diagnostic>) -> Design {
    let mut d = Design { kind, source: text.trim_end().to_string(), line, chart: None, screen: None, transitions: vec![], screens: vec![] };
    match kind {
        DesignKind::Flowchart => match parse_chart(text) {
            Ok(c) => d.chart = Some(c),
            Err((off, e)) => diags.push(Diagnostic { line: line + 1 + off, severity: Severity::Error, message: format!("flowchart: {e}") }),
        },
        DesignKind::Screen | DesignKind::Storyboard => match parse_yaml(text) {
            Ok(v) => {
                let r = if kind == DesignKind::Screen { parse_screen(&v).map(|s| d.screen = Some(s)) } else { parse_storyboard(&v).map(|t| d.transitions = t) };
                if let Err(e) = r {
                    diags.push(Diagnostic { line, severity: Severity::Error, message: format!("{} block: {e}", kind.as_str()) });
                }
            }
            Err(e) => diags.push(Diagnostic { line, severity: Severity::Error, message: format!("{} block is not valid YAML: {e}", kind.as_str()) }),
        },
    }
    d
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_unquoted_templates() {
        assert_eq!(quote_templates("lead_id: {{lead_id}}"), "lead_id: \"{{lead_id}}\"");
        assert_eq!(quote_templates("GET: {{env.API}}/quotes/{{quote_id}}"), "GET: \"{{env.API}}/quotes/{{quote_id}}\"");
        assert_eq!(quote_templates("json: { id: {{x}}, n: 2 }"), "json: { id: \"{{x}}\", n: 2 }");
        assert_eq!(quote_templates("email: guest+{{run.id}}@x.test"), "email: \"guest+{{run.id}}@x.test\"");
        assert_eq!(quote_templates("a: \"{{x}}\""), "a: \"{{x}}\"");
        assert_eq!(quote_templates("- goto: {{env.WEB}}/a # go"), "- goto: \"{{env.WEB}}/a\" # go");
        let v = parse_yaml("json: { lead_id: {{lead_id}}, fees: [ {{a}} ] }").unwrap();
        assert_eq!(v["json"]["lead_id"], "{{lead_id}}");
        assert_eq!(v["json"]["fees"][0], "{{a}}");
    }

    const DOC: &str = r#"---
id: quotes.accept
area: Quotes & payments
tags: [revenue, smoke]
---

# Client accepts a quote

Why it matters.

## Given

1. **A lead exists**
   ```http
   POST: {{env.API}}/leads
   json: { name: Jane }
   capture: { lead_id: $.id }
   ```

## When

2. **Jane accepts**
   ```browser
   - goto: {{env.WEB}}/accept/{{lead_id}}
   - click: { role: button, name: Accept }
   ```

## Then

3. **She sees a confirmation**
   ```expect
   text: Thanks
   ```

## Cleanup

- Remove the lead
  ```http
  DELETE: {{env.API}}/leads/{{lead_id}}
  ```
"#;

    #[test]
    fn parses_document() {
        let p = parse_flow("flows/quotes/accept.flow.md", DOC);
        assert!(!p.has_errors(), "{:?}", p.diagnostics);
        let f = p.flow;
        assert_eq!(f.id, "quotes.accept");
        assert_eq!(f.title, "Client accepts a quote");
        assert_eq!(f.why, "Why it matters.");
        assert_eq!(f.meta.tags, vec!["revenue", "smoke"]);
        assert_eq!(f.sections.len(), 4);
        let steps: Vec<_> = f.steps().collect();
        assert_eq!(steps[0].intent, "A lead exists");
        assert_eq!(steps[0].key, "a-lead-exists");
        assert_eq!(steps[0].number, Some(1));
        assert_eq!(steps[0].blocks[0].value["POST"], "{{env.API}}/leads");
        assert_eq!(steps[1].blocks[0].kind, BlockKind::Browser);
        assert_eq!(steps[3].intent, "Remove the lead");
        assert_eq!(steps[3].number, None);
        assert_eq!(steps[0].line, 13);
    }

    #[test]
    fn draft_flow_with_questions_and_notes() {
        let src = "---\nstage: draft\n---\n# Client pays a deposit\n\nWhy.\n\n## Rules\n\nDeposits are 10%.\n\n## Given\n\n1. **A quote of £1,000 exists**\n\n## Then\n\n2. **The client is asked for £100**\n\n## Open questions\n\n- Is VAT included\n  in the deposit?\n- Who can waive it?\n";
        let p = parse_flow("flows/x.flow.md", src);
        assert!(!p.has_errors(), "{:?}", p.diagnostics);
        assert!(p.diagnostics.is_empty(), "{:?}", p.diagnostics);
        assert_eq!(p.flow.meta.stage, Stage::Draft);
        assert_eq!(p.flow.questions, vec!["Is VAT included in the deposit?", "Who can waive it?"]);
        assert_eq!(p.flow.notes[0].heading, "Rules");
        assert_eq!(p.flow.notes[0].markdown, "Deposits are 10%.");
        assert_eq!(p.flow.unimplemented_steps(), 2);
    }

    #[test]
    fn reports_missing_title_and_bad_yaml() {
        let p = parse_flow("x.flow.md", "## Given\n\n1. **A**\n   ```http\n   POST: [\n   ```\n");
        assert!(p.has_errors());
        assert!(p.diagnostics.iter().any(|d| d.message.contains("Title")));
        assert!(p.diagnostics.iter().any(|d| d.message.contains("not valid YAML")));
    }
}

/// Check a step's tables against each other and against the `{{row.…}}` its
/// blocks use, and work out how linked tables point at the first one.
fn check_tables(mut tables: Vec<Table>, blocks: &[RawBlock], diags: &mut Vec<Diagnostic>) -> Vec<Table> {
    let err = |diags: &mut Vec<Diagnostic>, line: usize, message: String| diags.push(Diagnostic { line, severity: Severity::Error, message });
    for t in &tables {
        let mut seen = std::collections::HashSet::new();
        for c in &t.columns {
            if c.key.is_empty() {
                err(diags, t.line, format!("table column `{}` needs a name made of letters or digits", c.header));
            } else if !seen.insert(c.key.clone()) {
                err(diags, t.line, format!("two table columns are both `{}`; give them different names", c.key));
            }
        }
        if t.rows.is_empty() {
            err(diags, t.line, "table has no rows".into());
        }
    }
    if tables.len() > 1 {
        let driver_name = tables[0].name.clone();
        match &driver_name {
            None => err(diags, tables[0].line, "this step has several tables: name the first one with a `Table: <name>` line above it, so the others can link to it".into()),
            Some(dn) => {
                let dkey = column_key(dn);
                let ids: std::collections::HashSet<String> = tables[0].rows.iter().map(|r| r.cells.first().map(|c| c.trim().to_string()).unwrap_or_default()).collect();
                for t in tables.iter_mut().skip(1) {
                    let Some(name) = t.name.clone() else {
                        err(diags, t.line, format!("name this table with a `Table: <name>` line above it, so blocks can use its rows as `{{{{row.<name>}}}}`"));
                        continue;
                    };
                    match t.columns.iter().position(|c| c.key == dkey) {
                        None => err(
                            diags,
                            t.line,
                            format!("table `{name}` doesn't link to `{dn}`: add a column named `{dn}` holding the id (first column) of the `{dn}` row each line belongs to. Tables link one level deep, to the step's first table"),
                        ),
                        Some(i) => {
                            t.link = Some(i);
                            for r in &t.rows {
                                let id = r.cells.get(i).map(|c| c.trim()).unwrap_or("");
                                if !ids.contains(id) {
                                    err(diags, r.line, format!("`{id}` in the `{dn}` column of table `{name}` isn't the id of any row in `{dn}`"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // `{{row.x}}` must name a column of the first table, or a linked table.
    let known: Vec<String> = match tables.first() {
        Some(d) => d.columns.iter().map(|c| c.key.clone()).chain(tables.iter().skip(1).filter_map(|t| t.name.as_deref().map(column_key))).collect(),
        None => vec![],
    };
    for b in blocks {
        for v in crate::template::variables(&b.value) {
            let mut parts = v.split('.');
            if parts.next() != Some("row") {
                continue;
            }
            let col = parts.next().unwrap_or("");
            if tables.is_empty() {
                err(diags, b.line, format!("`{{{{{v}}}}}` refers to a table row, but this step has no table"));
            } else if !known.iter().any(|k| k == col) {
                err(diags, b.line, format!("`{{{{{v}}}}}`: the table has no column `{col}` (columns: {})", known.join(", ")));
            }
        }
    }
    tables
}
