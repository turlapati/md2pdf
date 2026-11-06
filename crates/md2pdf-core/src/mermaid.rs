//! Mermaid → SVG via the pure-Rust `mermaid-rs-renderer` crate.
//!
//! Isolated in its own module so the renderer can be swapped (e.g. for
//! `merman`, which targets stricter Mermaid.js parity) without touching the
//! translator.

use std::fmt;

#[derive(Debug)]
pub struct MermaidError(String);

impl fmt::Display for MermaidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MermaidError {}

pub fn render_svg(code: &str, font: &str) -> Result<String, MermaidError> {
    let code = code.trim();
    if code.is_empty() {
        return Err(MermaidError("empty diagram".into()));
    }
    // The renderer is fairly new; a panic inside it must not take the whole
    // document down. Catch it and degrade to a warning + source listing.
    let result = std::panic::catch_unwind(|| {
        let mut opts = mermaid_rs_renderer::RenderOptions::default();
        // Deterministic, host-independent text measurement. Without this the
        // renderer scans the machine's system fonts (fontdb) to measure label
        // widths, so node sizes would differ between laptops and be
        // unavailable under WASM.
        opts.layout.fast_text_metrics = true;
        // Use a family Typst embeds so the SVG text is shaped with the same
        // font the rest of the document uses (no silent fallback).
        opts.theme.font_family = format!("{font}, serif");
        mermaid_rs_renderer::render_with_options(code, opts)
    });
    match result {
        Ok(Ok(svg)) => Ok(sanitize(svg)),
        Ok(Err(e)) => Err(MermaidError(format!("{e:#}"))),
        Err(_) => Err(MermaidError("renderer panicked on this diagram".into())),
    }
}

/// Typst's SVG loader (usvg) is strict; strip things it does not need.
fn sanitize(svg: String) -> String {
    // Remove an XML declaration/BOM if present; usvg handles both but a BOM
    // in the middle of a Typst virtual file is unnecessary noise.
    svg.trim_start_matches('\u{feff}').to_string()
}
