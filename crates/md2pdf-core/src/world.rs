//! A minimal, fully in-memory [`typst::World`].
//!
//! Design goals:
//! * **No filesystem, no network, no system font scan.** Everything the
//!   compiler can see is handed to it explicitly: the generated main source,
//!   the rendered Mermaid SVGs, and the local image assets. This is what makes
//!   the same code run unchanged natively and under `wasm32-unknown-unknown`.
//! * **Deterministic fonts.** Only the fonts embedded via `typst-assets`
//!   (plus any the caller adds) are available, so a document renders the same
//!   on every machine.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// Fonts are parsed once per process and shared by every compilation.
pub struct FontSet {
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

static EMBEDDED: OnceLock<Arc<FontSet>> = OnceLock::new();

fn load_fonts(extra: &[Vec<u8>]) -> FontSet {
    let mut book = FontBook::new();
    let mut fonts = Vec::new();
    let embedded = typst_assets::fonts().map(Bytes::new);
    let extra = extra.iter().map(|b| Bytes::new(b.clone()));
    for data in embedded.chain(extra) {
        for font in Font::iter(data) {
            book.push(font.info().clone());
            fonts.push(font);
        }
    }
    FontSet { book: LazyHash::new(book), fonts }
}

/// Names of every font family available to the compiler (for diagnostics).
pub fn font_families(extra: &[Vec<u8>]) -> Vec<String> {
    let set = font_set(extra);
    set.book.families().map(|(name, _)| name.to_string()).collect()
}

fn font_set(extra: &[Vec<u8>]) -> Arc<FontSet> {
    if extra.is_empty() {
        EMBEDDED.get_or_init(|| Arc::new(load_fonts(&[]))).clone()
    } else {
        Arc::new(load_fonts(extra))
    }
}

fn vfile(path: &str) -> FileId {
    let vpath = VirtualPath::new(path).expect("generated virtual paths are always valid");
    FileId::new(RootedPath::new(VirtualRoot::Project, vpath))
}

/// Characters in `text` that no available font can draw (deduplicated, in
/// order of first appearance). Typst silently drops such glyphs, so hosts
/// surface these as a warning.
pub fn uncovered_chars(text: &str, extra: &[Vec<u8>]) -> Vec<char> {
    let set = font_set(extra);
    let mut seen = std::collections::BTreeSet::new();
    let mut missing = Vec::new();
    for ch in text.chars() {
        if ch.is_ascii() || ch.is_whitespace() || ch.is_control() || !seen.insert(ch) {
            continue;
        }
        // Variation selectors / joiners are not drawn on their own.
        if matches!(ch as u32, 0xFE00..=0xFE0F | 0x200B..=0x200D | 0x2060) {
            continue;
        }
        let covered = set.fonts.iter().any(|f| f.info().coverage.contains(ch as u32));
        if !covered {
            missing.push(ch);
        }
    }
    missing
}

pub struct MemWorld {
    library: LazyHash<Library>,
    fonts: Arc<FontSet>,
    main: FileId,
    sources: HashMap<FileId, Source>,
    files: HashMap<FileId, Bytes>,
    today: Option<Datetime>,
}

impl MemWorld {
    /// `main_src` is the generated Typst source. `files` maps absolute virtual
    /// paths (e.g. `/mermaid/0.svg`, `/assets/img.png`) to bytes.
    pub fn new(
        main_src: String,
        files: HashMap<String, Vec<u8>>,
        extra_fonts: &[Vec<u8>],
        today: Option<Datetime>,
    ) -> Self {
        let main = vfile("/main.typ");
        let mut sources = HashMap::new();
        sources.insert(main, Source::new(main, main_src));

        let files = files
            .into_iter()
            .map(|(p, b)| (vfile(&p), Bytes::new(b)))
            .collect();

        Self {
            library: LazyHash::new(Library::builder().build()),
            fonts: font_set(extra_fonts),
            main,
            sources,
            files,
            today,
        }
    }
}

impl World for MemWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.fonts.book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.sources
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::NotFound(id.vpath().get_with_slash().into()))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(b) = self.files.get(&id) {
            return Ok(b.clone());
        }
        if let Some(s) = self.sources.get(&id) {
            return Ok(Bytes::from_string(s.text().to_string()));
        }
        Err(FileError::NotFound(id.vpath().get_with_slash().into()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        // The caller supplies the date, so the core never touches the system
        // clock (which does not exist on wasm32-unknown-unknown).
        self.today
    }
}
