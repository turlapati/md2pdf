//! GitHub-Flavored Markdown → Typst source translation.
//!
//! We deliberately emit Typst *function-call* syntax (`#heading(..)[..]`,
//! `#list(..)`, `#table(..)`, `#raw("..")`) instead of Typst markup
//! (`= Title`, `- item`, `` `code` ``). Markup is whitespace- and
//! line-position-sensitive, which makes nested lists, loose lists, code
//! containing backticks, and prose containing `[`/`]` fragile. Function calls
//! have none of those problems; only prose runs need escaping (see
//! [`crate::escape::markup`]).

use std::collections::{BTreeSet, HashMap};

use pulldown_cmark::{
    Alignment, CodeBlockKind, Event, HeadingLevel, MetadataBlockKind, Options, Parser,
    Tag, TagEnd,
};

use crate::escape;
use crate::{Config, Paper, RemoteImages};

/// Result of translation: Typst source plus the virtual files it references.
pub struct Translated {
    pub source: String,
    pub files: HashMap<String, Vec<u8>>,
    pub warnings: Vec<String>,
    pub title: Option<String>,
}

pub fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_GFM
}

/// Local (non-URL) image paths referenced by the document, as written.
/// Hosts (CLI, VS Code) read these from disk and pass the bytes to
/// [`crate::render`]; the core itself never touches a filesystem.
pub fn local_asset_paths(md: &str) -> Vec<String> {
    let mut set = BTreeSet::new();
    for ev in Parser::new_ext(md, parser_options()) {
        if let Event::Start(Tag::Image { dest_url, .. }) = ev {
            if is_local_path(&dest_url) {
                set.insert(dest_url.to_string());
            }
        }
    }
    set.into_iter().collect()
}

/// `http(s)://` image URLs referenced by the document, as written. With
/// [`crate::RemoteImages::Embed`], hosts download these and pass the bytes in
/// `assets` keyed by URL.
pub fn remote_image_urls(md: &str) -> Vec<String> {
    let mut set = BTreeSet::new();
    for ev in Parser::new_ext(md, parser_options()) {
        if let Event::Start(Tag::Image { dest_url, .. }) = ev {
            if is_http_url(&dest_url) {
                set.insert(dest_url.to_string());
            }
        }
    }
    set.into_iter().collect()
}

fn is_http_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Image format from magic bytes; remote URLs often have no usable extension.
fn sniff_image_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    if head.contains("<svg") {
        return Some("svg");
    }
    None
}

fn is_local_path(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    !(lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
        || lower.starts_with("ftp://")
        || lower.starts_with("//"))
}

#[derive(Clone, Copy)]
enum ListKind {
    Bullet,
    Ordered(u64),
}

struct ListState {
    kind: ListKind,
    loose: bool,
    item_count: usize,
}

pub struct Translator<'a> {
    cfg: &'a Config,
    assets: &'a HashMap<String, Vec<u8>>,
    /// Stack of output buffers; `bufs.last_mut()` is the active one.
    bufs: Vec<String>,
    files: HashMap<String, Vec<u8>>,
    warnings: Vec<String>,
    mermaid_count: usize,
    asset_count: usize,
    lists: Vec<ListState>,
    /// Footnote label → already-translated Typst content.
    footnotes: HashMap<String, String>,
    /// Heading slugs (GitHub style, de-duplicated) for `[x](#anchor)` links.
    heading_labels: Vec<String>,
    heading_idx: usize,
    known_labels: BTreeSet<String>,
    /// Text of the heading currently being emitted (for the document title).
    current_heading: Option<String>,
    first_h1: Option<String>,
    /// Raw code-block accumulation.
    code: Option<(String, String)>,
    /// Inside an image tag the alt-text events are collected, not emitted.
    image_depth: usize,
    /// (destination, title, alt text) of the image being collected.
    pending_image: Option<(String, String, String)>,
    link_depth: usize,
    in_metadata: bool,
    metadata_title: Option<String>,
    html_warned: bool,
}

impl<'a> Translator<'a> {
    pub fn new(cfg: &'a Config, assets: &'a HashMap<String, Vec<u8>>) -> Self {
        Self {
            cfg,
            assets,
            bufs: vec![String::new()],
            files: HashMap::new(),
            warnings: Vec::new(),
            mermaid_count: 0,
            asset_count: 0,
            lists: Vec::new(),
            footnotes: HashMap::new(),
            heading_labels: Vec::new(),
            heading_idx: 0,
            known_labels: BTreeSet::new(),
            current_heading: None,
            first_h1: None,
            code: None,
            image_depth: 0,
            pending_image: None,
            link_depth: 0,
            in_metadata: false,
            metadata_title: None,
            html_warned: false,
        }
    }

    fn out(&mut self) -> &mut String {
        self.bufs.last_mut().expect("buffer stack never empty")
    }

    fn push_buf(&mut self) {
        self.bufs.push(String::new());
    }

    fn pop_buf(&mut self) -> String {
        self.bufs.pop().expect("unbalanced buffer stack")
    }

    fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    pub fn translate(mut self, md: &str) -> Translated {
        let events: Vec<Event<'_>> = Parser::new_ext(md, parser_options()).collect();

        // Pass 1a: heading labels (for in-document anchors).
        self.collect_heading_labels(&events);

        // Pass 1b: footnote definitions, translated up-front so references can
        // inline them as `#footnote[...]`.
        let body: Vec<Event<'_>> = self.extract_footnotes(events);

        // Pass 2: the body.
        self.push_buf();
        self.emit_events(&body);
        let body_src = self.pop_buf();

        let title = self
            .cfg
            .title
            .clone()
            .or_else(|| self.metadata_title.clone())
            .or_else(|| self.first_h1.clone());

        let mut source = self.preamble(title.as_deref());
        source.push_str(&body_src);

        Translated {
            source,
            files: self.files,
            warnings: self.warnings,
            title,
        }
    }

    // ------------------------------------------------------------------
    // Pre-passes
    // ------------------------------------------------------------------

    fn collect_heading_labels(&mut self, events: &[Event<'_>]) {
        let mut in_heading = false;
        let mut text = String::new();
        let mut seen: HashMap<String, usize> = HashMap::new();
        for ev in events {
            match ev {
                Event::Start(Tag::Heading { .. }) => {
                    in_heading = true;
                    text.clear();
                }
                Event::End(TagEnd::Heading(_)) => {
                    in_heading = false;
                    let base = escape::slug(&text);
                    let n = seen.entry(base.clone()).or_insert(0);
                    let label = if *n == 0 { base.clone() } else { format!("{base}-{n}") };
                    *n += 1;
                    let label = escape::label_safe(&label);
                    self.known_labels.insert(label.clone());
                    self.heading_labels.push(label);
                }
                Event::Text(t) | Event::Code(t) if in_heading => text.push_str(t),
                _ => {}
            }
        }
    }

    fn extract_footnotes<'e>(&mut self, events: Vec<Event<'e>>) -> Vec<Event<'e>> {
        let mut body = Vec::with_capacity(events.len());
        let mut current: Option<(String, Vec<Event<'e>>)> = None;
        let mut depth = 0usize;
        for ev in events {
            match (&ev, &mut current) {
                (Event::Start(Tag::FootnoteDefinition(label)), None) => {
                    current = Some((label.to_string(), Vec::new()));
                    depth = 1;
                }
                (Event::Start(Tag::FootnoteDefinition(_)), Some((_, buf))) => {
                    depth += 1;
                    buf.push(ev);
                }
                (Event::End(TagEnd::FootnoteDefinition), Some((label, buf))) => {
                    depth -= 1;
                    if depth == 0 {
                        let label = label.clone();
                        let buf = std::mem::take(buf);
                        current = None;
                        self.push_buf();
                        self.emit_events(&buf);
                        let content = self.pop_buf();
                        self.footnotes.insert(label, content.trim().to_string());
                    } else {
                        buf.push(ev);
                    }
                }
                (_, Some((_, buf))) => buf.push(ev),
                (_, None) => body.push(ev),
            }
        }
        body
    }

    // ------------------------------------------------------------------
    // Preamble
    // ------------------------------------------------------------------

    fn preamble(&self, title: Option<&str>) -> String {
        let paper = match self.cfg.paper {
            Paper::A4 => "a4",
            Paper::UsLetter => "us-letter",
        };
        let mut p = String::new();
        p.push_str("// Generated by md2pdf — do not edit.\n");
        if let Some(t) = title {
            p.push_str(&format!("#set document(title: \"{}\")\n", escape::string(t)));
        }
        p.push_str(&format!(
            "#set page(paper: \"{paper}\", margin: (x: 1.8cm, y: 2cm), numbering: \"1 / 1\")\n"
        ));
        p.push_str(&format!(
            "#set text(font: \"{}\", size: {}pt, lang: \"en\")\n",
            escape::string(&self.cfg.body_font),
            self.cfg.font_size
        ));
        p.push_str("#set par(justify: false, leading: 0.65em)\n");
        if self.cfg.number_headings {
            p.push_str("#set heading(numbering: \"1.1\")\n");
        }
        p.push_str("#show heading: set block(above: 1.4em, below: 0.8em)\n");
        p.push_str(&format!(
            "#show raw: set text(font: \"{}\", size: 0.88em)\n",
            escape::string(&self.cfg.mono_font)
        ));
        p.push_str(
            "#show raw.where(block: true): it => block(\n  fill: luma(246), inset: 10pt, radius: 4pt, width: 100%,\n  stroke: 0.5pt + luma(210), breakable: true, it)\n",
        );
        p.push_str("#show raw.where(block: false): it => box(fill: luma(240), inset: (x: 3pt, y: 0pt), outset: (y: 3pt), radius: 2pt, it)\n");
        p.push_str("#show link: set text(fill: rgb(\"#1a5fb4\"))\n");
        p.push_str("#set table(stroke: 0.5pt + luma(200), inset: 6pt)\n");
        p.push_str("#show table.cell.where(y: 0): strong\n");
        p.push_str("#show table.cell.where(y: 0): set table.cell(fill: luma(240))\n");
        p.push_str(
            "#show quote.where(block: true): it => block(\n  stroke: (left: 3pt + luma(200)), inset: (left: 1em, y: 0.4em), width: 100%, it.body)\n",
        );
        p.push_str("#set list(indent: 0.6em)\n#set enum(indent: 0.6em)\n");
        // Scale images down to the available width, never up.
        p.push_str(
            "#let fit-image(path, ..args) = layout(size => {\n  let img = image(path, ..args)\n  let s = measure(img)\n  if s.width > size.width { image(path, width: 100%, ..args) } else { img }\n})\n",
        );
        // Task-list checkboxes drawn with primitives (no emoji font needed).
        p.push_str(
            "#let task(done) = box(baseline: 15%, width: 0.85em, height: 0.85em, radius: 2pt,\n  stroke: 0.6pt + luma(110), fill: if done { rgb(\"#2563eb\") } else { white },\n  align(center + horizon, if done { text(fill: white, size: 0.65em, weight: \"bold\")[$checkmark$] } else { none }))\n",
        );
        // GitHub-style alerts: > [!NOTE], [!TIP], [!IMPORTANT], [!WARNING], [!CAUTION]
        p.push_str(
            "#let alert(label, color, body) = block(\n  stroke: (left: 3pt + color), inset: (left: 1em, y: 0.5em), width: 100%, breakable: true,\n  [#text(fill: color, weight: \"bold\", size: 0.95em)[#label]\n\n#body])\n",
        );
        if self.cfg.toc {
            p.push_str("#outline(title: \"Contents\", indent: 1.2em)\n#pagebreak(weak: true)\n");
        }
        p.push('\n');
        p
    }

    // ------------------------------------------------------------------
    // Event emission
    // ------------------------------------------------------------------

    fn emit_events(&mut self, events: &[Event<'_>]) {
        for ev in events {
            self.emit(ev);
        }
    }

    fn emit(&mut self, ev: &Event<'_>) {
        // While inside an image tag, the alt text arrives as ordinary events;
        // collect its plain text and emit the image once the tag closes.
        if self.image_depth > 0 {
            match ev {
                Event::Start(Tag::Image { .. }) => self.image_depth += 1,
                Event::End(TagEnd::Image) => {
                    self.image_depth -= 1;
                    if self.image_depth == 0 {
                        if let Some((dest, title, alt)) = self.pending_image.take() {
                            self.image(&dest, &title, alt.trim());
                        }
                    }
                }
                Event::Text(t) | Event::Code(t) => {
                    if let Some((_, _, alt)) = &mut self.pending_image {
                        alt.push_str(t);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some((_, _, alt)) = &mut self.pending_image {
                        alt.push(' ');
                    }
                }
                _ => {}
            }
            return;
        }
        if self.in_metadata {
            if let Event::End(TagEnd::MetadataBlock(_)) = ev {
                self.in_metadata = false;
            } else if let Event::Text(t) = ev {
                self.scan_metadata(t);
            }
            return;
        }
        if let Some((_, buf)) = &mut self.code {
            match ev {
                Event::Text(t) => buf.push_str(t),
                Event::End(TagEnd::CodeBlock) => self.end_code_block(),
                _ => {}
            }
            return;
        }

        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                let s = escape::markup(t);
                if let Some(h) = &mut self.current_heading {
                    h.push_str(t);
                }
                self.out().push_str(&s);
            }
            Event::Code(c) => {
                let s = format!("#raw(\"{}\");", escape::string(c));
                if let Some(h) = &mut self.current_heading {
                    h.push_str(c);
                }
                self.out().push_str(&s);
            }
            Event::SoftBreak => self.out().push(' '),
            Event::HardBreak => self.out().push_str("#linebreak();"),
            Event::Rule => self
                .out()
                .push_str("\n#line(length: 100%, stroke: 0.5pt + luma(190))\n\n"),
            Event::TaskListMarker(done) => {
                let s = format!("#task({}); ", if *done { "true" } else { "false" });
                self.out().push_str(&s);
            }
            Event::FootnoteReference(label) => {
                let content = self.footnotes.get(label.as_ref()).cloned();
                match content {
                    Some(c) => {
                        let s = format!("#footnote[{c}];");
                        self.out().push_str(&s);
                    }
                    None => {
                        self.warn(format!("footnote reference [^{label}] has no definition"));
                        let s = format!("#super[{}];", escape::markup(label));
                        self.out().push_str(&s);
                    }
                }
            }
            Event::Html(h) | Event::InlineHtml(h) => self.html(h),
            Event::InlineMath(m) | Event::DisplayMath(m) => {
                // LaTeX math is not Typst math; keep it verbatim as code.
                let s = format!("#raw(\"{}\");", escape::string(m));
                self.out().push_str(&s);
            }
        }
    }

    fn html(&mut self, h: &str) {
        let t = h.trim().to_ascii_lowercase();
        if t == "<br>" || t == "<br/>" || t == "<br />" {
            self.out().push_str("#linebreak();");
        } else if t.starts_with("<!--") {
            // comments: drop silently
        } else {
            if !self.html_warned {
                self.warn("raw HTML is not rendered (dropped). Use Markdown syntax instead.");
                self.html_warned = true;
            }
        }
    }

    fn scan_metadata(&mut self, text: &str) {
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("title:") {
                let v = rest.trim().trim_matches('"').trim_matches('\'');
                if !v.is_empty() {
                    self.metadata_title = Some(v.to_string());
                }
            }
        }
    }

    fn start(&mut self, tag: &Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                let lvl = heading_level(*level);
                self.current_heading = Some(String::new());
                let s = format!("\n#heading(level: {lvl})[");
                self.out().push_str(&s);
            }
            Tag::BlockQuote(kind) => {
                use pulldown_cmark::BlockQuoteKind as K;
                let s = match kind {
                    None => "\n#quote(block: true)[\n".to_string(),
                    Some(k) => {
                        let (label, color) = match k {
                            K::Note => ("Note", "#1f6feb"),
                            K::Tip => ("Tip", "#1a7f37"),
                            K::Important => ("Important", "#8250df"),
                            K::Warning => ("Warning", "#9a6700"),
                            K::Caution => ("Caution", "#cf222e"),
                        };
                        format!("\n#alert(\"{label}\", rgb(\"{color}\"))[\n")
                    }
                };
                self.out().push_str(&s);
            }
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::HtmlBlock => {}
            Tag::List(start) => {
                let kind = match start {
                    Some(n) => ListKind::Ordered(*n),
                    None => ListKind::Bullet,
                };
                self.lists.push(ListState { kind, loose: false, item_count: 0 });
                self.push_buf();
            }
            Tag::Item => {
                if let Some(l) = self.lists.last_mut() {
                    l.item_count += 1;
                }
                self.out().push_str("\n  [");
            }
            Tag::FootnoteDefinition(_) => {} // handled in pre-pass
            Tag::DefinitionList => self.out().push_str("\n#terms(\n"),
            Tag::DefinitionListTitle => self.out().push_str("  (["),
            Tag::DefinitionListDefinition => self.out().push_str(", ["),
            Tag::Table(aligns) => {
                let cols = aligns.len().max(1);
                let align: Vec<&str> = aligns
                    .iter()
                    .map(|a| match a {
                        Alignment::Center => "center",
                        Alignment::Right => "right",
                        _ => "left",
                    })
                    .collect();
                let s = format!(
                    "\n#table(\n  columns: {cols},\n  align: ({},),\n",
                    align.join(", ")
                );
                self.out().push_str(&s);
            }
            Tag::TableHead => self.out().push_str("  table.header(\n"),
            Tag::TableRow => {}
            Tag::TableCell => self.out().push_str("    ["),
            Tag::Emphasis => self.out().push_str("#emph["),
            Tag::Strong => self.out().push_str("#strong["),
            Tag::Strikethrough => self.out().push_str("#strike["),
            Tag::Superscript => self.out().push_str("#super["),
            Tag::Subscript => self.out().push_str("#sub["),
            Tag::Link { dest_url, .. } => {
                self.link_depth += 1;
                let dest = dest_url.to_string();
                if let Some(anchor) = dest.strip_prefix('#') {
                    let label = escape::label_safe(&escape::slug(anchor));
                    if self.known_labels.contains(&label) {
                        let s = format!("#link(<{label}>)[");
                        self.out().push_str(&s);
                    } else {
                        self.warn(format!("link target #{anchor} does not match any heading"));
                        self.out().push_str("#text[");
                    }
                } else if is_local_path(&dest) {
                    // Relative file links can't resolve inside a PDF; show as text.
                    self.out().push_str("#text(fill: rgb(\"#1a5fb4\"))[");
                } else {
                    let s = format!("#link(\"{}\")[", escape::string(&dest));
                    self.out().push_str(&s);
                }
            }
            Tag::Image { dest_url, title, .. } => {
                self.image_depth = 1;
                self.pending_image = Some((dest_url.to_string(), title.to_string(), String::new()));
            }
            Tag::MetadataBlock(MetadataBlockKind::YamlStyle)
            | Tag::MetadataBlock(MetadataBlockKind::PlusesStyle) => {
                self.in_metadata = true;
            }
        }
    }

    fn end(&mut self, tag: &TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if let Some(l) = self.lists.last_mut() {
                    l.loose = true;
                }
                self.out().push_str("\n\n");
            }
            TagEnd::Heading(_) => {
                let text = self.current_heading.take().unwrap_or_default();
                let label = self
                    .heading_labels
                    .get(self.heading_idx)
                    .cloned()
                    .unwrap_or_default();
                self.heading_idx += 1;
                if self.first_h1.is_none() && matches!(tag, TagEnd::Heading(HeadingLevel::H1)) {
                    self.first_h1 = Some(text.trim().to_string());
                }
                let s = if label.is_empty() {
                    "]\n\n".to_string()
                } else {
                    format!("] <{label}>\n\n")
                };
                self.out().push_str(&s);
            }
            TagEnd::BlockQuote(_) => self.out().push_str("]\n\n"),
            TagEnd::CodeBlock => {} // handled in emit()
            TagEnd::HtmlBlock => {}
            TagEnd::List(_) => {
                let body = self.pop_buf();
                let state = self.lists.pop().expect("list stack");
                let tight = if state.loose { "false" } else { "true" };
                let head = match state.kind {
                    ListKind::Bullet => format!("#list(tight: {tight},"),
                    ListKind::Ordered(start) => format!("#enum(tight: {tight}, start: {start},"),
                };
                let s = format!("\n{head}{body}\n)\n\n");
                self.out().push_str(&s);
            }
            TagEnd::Item => {
                // Trim trailing paragraph breaks inside the item so tight lists
                // don't get extra space.
                let o = self.out();
                while o.ends_with('\n') {
                    o.pop();
                }
                o.push_str("],");
            }
            TagEnd::FootnoteDefinition => {}
            TagEnd::DefinitionList => self.out().push_str(")\n\n"),
            TagEnd::DefinitionListTitle => self.out().push(']'),
            TagEnd::DefinitionListDefinition => self.out().push_str("]),\n"),
            TagEnd::Table => self.out().push_str(")\n\n"),
            TagEnd::TableHead => self.out().push_str("  ),\n"),
            TagEnd::TableRow => self.out().push('\n'),
            TagEnd::TableCell => self.out().push_str("],"),
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript => self.out().push_str("];"),
            TagEnd::Link => {
                self.link_depth = self.link_depth.saturating_sub(1);
                self.out().push_str("];");
            }
            TagEnd::Image => {}
            TagEnd::MetadataBlock(_) => {}
        }
    }

    fn end_code_block(&mut self) {
        let (lang, code) = self.code.take().expect("code block state");
        let code = code.strip_suffix('\n').unwrap_or(&code).to_string();
        if lang.eq_ignore_ascii_case("mermaid") {
            self.mermaid(&code);
            return;
        }
        let lang_arg = if lang.is_empty() {
            String::new()
        } else {
            format!("lang: \"{}\", ", escape::string(&lang))
        };
        let s = format!("\n#raw(block: true, {lang_arg}\"{}\")\n\n", escape::string(&code));
        self.out().push_str(&s);
    }

    fn mermaid(&mut self, code: &str) {
        match crate::mermaid::render_svg(code, &self.cfg.body_font) {
            Ok(svg) => {
                let path = format!("/mermaid/{}.svg", self.mermaid_count);
                self.mermaid_count += 1;
                self.files.insert(path.clone(), svg.into_bytes());
                let s = format!("\n#align(center, fit-image(\"{path}\"))\n\n");
                self.out().push_str(&s);
            }
            Err(e) => {
                self.warn(format!("mermaid diagram could not be rendered: {e}"));
                let s = format!(
                    "\n#block(fill: rgb(\"#fff4e5\"), stroke: 0.5pt + rgb(\"#f0a000\"), inset: 8pt, radius: 4pt, width: 100%)[#text(size: 0.85em, fill: rgb(\"#7a4b00\"))[Mermaid diagram not rendered: {}]]\n#raw(block: true, \"{}\")\n\n",
                    escape::markup(&e.to_string()),
                    escape::string(code)
                );
                self.out().push_str(&s);
            }
        }
    }

    fn image(&mut self, dest: &str, title: &str, alt: &str) {
        if !is_local_path(dest) {
            self.remote_image(dest, alt);
            return;
        }
        let Some(bytes) = self.assets.get(dest) else {
            self.warn(format!("image not found: {dest}"));
            let s = format!("#text(fill: red)[\\[missing image: {}\\]];", escape::markup(dest));
            self.out().push_str(&s);
            return;
        };
        let ext = dest
            .rsplit('.')
            .next()
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        let ok = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp");
        if !ok {
            self.warn(format!("unsupported image format: {dest}"));
            let s = format!("#text(fill: red)[\\[unsupported image: {}\\]];", escape::markup(dest));
            self.out().push_str(&s);
            return;
        }
        let path = self.add_asset(bytes.clone(), &ext);
        self.block_image(&path, title);
    }

    /// Remote images are never fetched by the core. With `RemoteImages::Embed`
    /// the host may have supplied the bytes keyed by URL; otherwise the alt
    /// text is shown, linked to the image URL unless already inside a link.
    fn remote_image(&mut self, dest: &str, alt: &str) {
        let fetched = match self.cfg.remote_images {
            RemoteImages::Embed => self
                .assets
                .get(dest)
                .and_then(|b| sniff_image_format(b).map(|ext| (b.clone(), ext))),
            RemoteImages::AltText => None,
        };
        if let Some((bytes, ext)) = fetched {
            let path = self.add_asset(bytes, ext);
            if self.link_depth > 0 {
                // Typically a README badge: keep it inline at text height.
                let s = format!("#box(baseline: 20%, image(\"{path}\", height: 1.4em));");
                self.out().push_str(&s);
            } else {
                self.block_image(&path, "");
            }
            return;
        }
        let label = escape::markup(if alt.is_empty() { dest } else { alt });
        let s = if self.link_depth > 0 {
            label
        } else {
            format!("#link(\"{}\")[{label}];", escape::string(dest))
        };
        self.out().push_str(&s);
    }

    fn add_asset(&mut self, bytes: Vec<u8>, ext: &str) -> String {
        let path = format!("/assets/{}.{ext}", self.asset_count);
        self.asset_count += 1;
        self.files.insert(path.clone(), bytes);
        path
    }

    fn block_image(&mut self, path: &str, title: &str) {
        let s = if title.trim().is_empty() {
            format!("\n#align(center, fit-image(\"{path}\"))\n\n")
        } else {
            format!(
                "\n#figure(fit-image(\"{path}\"), caption: [{}])\n\n",
                escape::markup(title)
            )
        };
        self.out().push_str(&s);
    }
}

fn heading_level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}
