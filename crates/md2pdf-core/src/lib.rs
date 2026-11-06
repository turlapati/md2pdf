//! # md2pdf-core
//!
//! GitHub-Flavored Markdown + Mermaid → PDF, with no runtime dependencies:
//! no Node, no browser, no LaTeX, no Pandoc, no system fonts.
//!
//! Pipeline: `pulldown-cmark` (GFM AST) → Typst source generation →
//! `mermaid-rs-renderer` for ```mermaid fences → in-memory Typst `World`
//! with embedded fonts → `typst-pdf`.
//!
//! The crate is `std`-only and free of filesystem/network/clock access so it
//! compiles unchanged to `wasm32-unknown-unknown`. Hosts (CLI, VS Code
//! extension) supply image bytes and the current date.

mod escape;
mod markdown;
mod mermaid;
mod world;

use std::collections::HashMap;

use typst::foundations::Datetime;
use typst_layout::PagedDocument;

pub use markdown::{local_asset_paths, remote_image_urls};
pub use world::font_families;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Paper {
    #[default]
    A4,
    UsLetter,
}

impl std::str::FromStr for Paper {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "a4" => Ok(Paper::A4),
            "letter" | "us-letter" | "us_letter" | "usletter" => Ok(Paper::UsLetter),
            other => Err(format!("unknown paper size '{other}' (use a4 or letter)")),
        }
    }
}

/// How images with an `http(s)://` URL are rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RemoteImages {
    /// Render the alt text as a link to the image URL. Nothing is fetched.
    #[default]
    AltText,
    /// Embed the image if the host supplied its bytes in `assets` (keyed by
    /// URL, see [`remote_image_urls`]); otherwise fall back to alt text.
    Embed,
}

impl std::str::FromStr for RemoteImages {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
            "alttext" | "alt" => Ok(RemoteImages::AltText),
            "embed" => Ok(RemoteImages::Embed),
            _ => Err(format!("unknown remote-images mode '{s}' (use alt-text or embed)")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub paper: Paper,
    /// Body font size in points.
    pub font_size: f32,
    /// Insert a table of contents before the body.
    pub toc: bool,
    /// Number headings 1., 1.1, …
    pub number_headings: bool,
    /// Explicit document title (else YAML `title:` else first `# H1`).
    pub title: Option<String>,
    /// Font family names; must exist among embedded + `extra_fonts`.
    pub body_font: String,
    pub mono_font: String,
    /// Additional font files (TTF/OTF bytes) made available to Typst.
    pub extra_fonts: Vec<Vec<u8>>,
    /// Date used for `datetime.today()` and PDF metadata (year, month, day).
    pub today: Option<(i32, u8, u8)>,
    pub remote_images: RemoteImages,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            paper: Paper::A4,
            font_size: 10.5,
            toc: false,
            number_headings: false,
            title: None,
            body_font: "Libertinus Serif".into(),
            mono_font: "DejaVu Sans Mono".into(),
            extra_fonts: Vec::new(),
            today: None,
            remote_images: RemoteImages::AltText,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Typst compilation failed:\n{0}")]
    Compile(String),
    #[error("PDF export failed:\n{0}")]
    Export(String),
}

pub struct Output {
    pub pdf: Vec<u8>,
    /// Non-fatal problems (dropped HTML, missing images, Mermaid failures…).
    pub warnings: Vec<String>,
    pub pages: usize,
    pub title: Option<String>,
}

/// Translate Markdown to Typst source without compiling (for debugging and
/// for users who want to hand-tune the output in Typst).
pub fn to_typst(md: &str, cfg: &Config, assets: &HashMap<String, Vec<u8>>) -> (String, Vec<String>) {
    let t = markdown::Translator::new(cfg, assets).translate(md);
    (t.source, t.warnings)
}

/// Render Markdown to PDF bytes.
///
/// `assets` maps each path returned by [`local_asset_paths`] (verbatim, as
/// written in the Markdown) to the file's bytes, and — with
/// [`RemoteImages::Embed`] — each URL from [`remote_image_urls`] to the
/// downloaded bytes. Hosts report fetch failures themselves.
pub fn render(md: &str, cfg: &Config, assets: &HashMap<String, Vec<u8>>) -> Result<Output, Error> {
    let t = markdown::Translator::new(cfg, assets).translate(md);
    let mut warnings = t.warnings;

    let missing = world::uncovered_chars(md, &cfg.extra_fonts);
    if !missing.is_empty() {
        let sample: String = missing.iter().take(12).collect();
        warnings.push(format!(
            "{} character(s) have no glyph in the embedded fonts and will be missing from the PDF (e.g. {sample}); add a font that covers them with --font-dir",
            missing.len()
        ));
    }

    let today = cfg
        .today
        .and_then(|(y, m, d)| Datetime::from_ymd(y, m, d));
    let world = world::MemWorld::new(t.source, t.files, &cfg.extra_fonts, today);

    let compiled = typst::compile::<PagedDocument>(&world);
    for w in &compiled.warnings {
        warnings.push(format!("typst: {}", w.message));
    }
    let doc = compiled
        .output
        .map_err(|errs| Error::Compile(format_diagnostics(&errs, &world)))?;

    let opts = typst_pdf::PdfOptions::default();
    let pdf = typst_pdf::pdf(&doc, &opts)
        .map_err(|errs| Error::Export(format_diagnostics(&errs, &world)))?;

    Ok(Output {
        pdf,
        warnings,
        pages: doc.pages().len(),
        title: t.title,
    })
}

fn format_diagnostics(errs: &typst::ecow::EcoVec<typst::diag::SourceDiagnostic>, world: &world::MemWorld) -> String {
    use typst::{World, WorldExt};
    let main = world.source(world.main()).ok();
    let mut out = String::new();
    for e in errs {
        let loc = world
            .range(e.span)
            .and_then(|r| main.as_ref()?.lines().byte_to_line(r.start))
            .map(|l| format!(" (generated Typst line {})", l + 1))
            .unwrap_or_default();
        out.push_str(&format!("- {}{loc}\n", e.message));
        for h in &e.hints {
            out.push_str(&format!("  hint: {}\n", h.v));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(md: &str) -> Output {
        render(md, &Config::default(), &HashMap::new()).expect("render")
    }

    #[test]
    fn renders_basic_document() {
        let out = run("# Title\n\nHello *world* with `code` and [a link](https://example.com).\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(out.pdf.starts_with(b"%PDF"));
        assert_eq!(out.pages, 1);
        assert_eq!(out.title.as_deref(), Some("Title"));
    }

    #[test]
    fn hostile_prose_does_not_break_typst() {
        let md = "# T\n\nText with #hash, @at, [brackets], *stars*, _under_, `tick`, $dollar, <angle>, ~tilde, -- dashes, // slashes, https://x.y/z and \\backslash.\n\n- item one\n  - nested `a\\b`\n- [ ] todo\n- [x] done\n\n1. one\n2. two\n\n> quote\n\n```rust\nfn main() { println!(\"```\"); }\n```\n";
        let out = run(md);
        assert!(out.pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn warns_about_uncovered_glyphs() {
        let out = run("Latin é is fine; 日本語 is not.\n");
        assert!(out.warnings.iter().any(|w| w.contains("no glyph")), "{:?}", out.warnings);
        let clean = run("Plain Latin, Ελληνικά, Кириллица — “quotes”.\n");
        assert!(!clean.warnings.iter().any(|w| w.contains("no glyph")), "{:?}", clean.warnings);
    }

    #[test]
    fn typst_code_in_prose_is_inert() {
        let src = to_typst("#set page(paper: \"a0\") #{panic(\"x\")}\n", &Config::default(), &HashMap::new()).0;
        assert!(src.contains("\\#set page"), "{src}");
        let out = run("#set page(paper: \"a0\")\n");
        assert_eq!(out.pages, 1);
    }

    const BADGE_MD: &str = "[![ci](https://example.com/ci.svg)](https://example.com/actions)\n\n![logo](https://example.com/logo)\n";
    const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="10"><rect width="40" height="10" fill="green"/></svg>"#;

    #[test]
    fn remote_images_default_to_linked_alt_text() {
        let out = run(BADGE_MD);
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        let src = to_typst(BADGE_MD, &Config::default(), &HashMap::new()).0;
        assert!(src.contains("#link(\"https://example.com/actions\")[ci];"), "{src}");
        assert!(src.contains("#link(\"https://example.com/logo\")[logo];"), "{src}");
        assert!(!src.contains("image(\""), "{src}");
    }

    #[test]
    fn remote_images_embed_supplied_bytes() {
        assert_eq!(
            remote_image_urls(BADGE_MD),
            vec!["https://example.com/ci.svg".to_string(), "https://example.com/logo".to_string()]
        );
        let cfg = Config { remote_images: RemoteImages::Embed, ..Default::default() };
        let mut assets = HashMap::new();
        assets.insert("https://example.com/ci.svg".to_string(), SVG.to_vec());
        let src = to_typst(BADGE_MD, &cfg, &assets).0;
        assert!(src.contains("#box(baseline: 20%, image(\"/assets/0.svg\""), "{src}");
        // Not supplied (fetch failed): falls back to alt text.
        assert!(src.contains("#link(\"https://example.com/logo\")[logo];"), "{src}");
        let out = render(BADGE_MD, &cfg, &assets).expect("render");
        assert!(out.pdf.starts_with(b"%PDF"));
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    }

    #[test]
    fn mermaid_renders_or_degrades() {
        let out = run("```mermaid\nflowchart LR\n  A[Start] --> B{Ok?}\n  B -->|yes| C[Done]\n```\n");
        assert!(out.pdf.starts_with(b"%PDF"));
    }
}
