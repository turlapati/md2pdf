use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;
use md2pdf_core::{Config, Paper};

/// Markdown (GFM) + Mermaid → PDF. Single binary, no runtime dependencies.
#[derive(Parser, Debug)]
#[command(name = "md2pdf", version, about)]
struct Cli {
    /// Input Markdown file (use "-" for stdin)
    #[arg(required_unless_present = "list_fonts")]
    input: Option<PathBuf>,

    /// Output PDF path (default: input with .pdf extension; "-" for stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Paper size: a4 or letter
    #[arg(long, default_value = "a4")]
    paper: String,

    /// Body font size in pt
    #[arg(long, default_value_t = 10.5)]
    font_size: f32,

    /// Insert a table of contents
    #[arg(long)]
    toc: bool,

    /// Number headings (1, 1.1, …)
    #[arg(long)]
    number_headings: bool,

    /// Document title (default: YAML front-matter title, else first H1)
    #[arg(long)]
    title: Option<String>,

    /// Body font family (must be embedded or supplied via --font-dir)
    #[arg(long, default_value = "Libertinus Serif")]
    body_font: String,

    /// Monospace font family
    #[arg(long, default_value = "DejaVu Sans Mono")]
    mono_font: String,

    /// Directory of extra .ttf/.otf fonts to make available (not recursive)
    #[arg(long)]
    font_dir: Option<PathBuf>,

    /// Write the generated Typst source instead of a PDF (for debugging)
    #[arg(long)]
    emit_typst: bool,

    /// List available font families and exit
    #[arg(long)]
    list_fonts: bool,

    /// Suppress warnings
    #[arg(short, long)]
    quiet: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let extra_fonts = match &cli.font_dir {
        Some(dir) => load_fonts(dir)?,
        None => Vec::new(),
    };

    if cli.list_fonts {
        for f in md2pdf_core::font_families(&extra_fonts) {
            println!("{f}");
        }
        return Ok(());
    }

    let input = cli.input.clone().expect("clap enforces INPUT unless --list-fonts");
    let (md, base_dir) = if input.as_os_str() == "-" {
        let mut s = String::new();
        io::stdin().read_to_string(&mut s)?;
        (s, PathBuf::from("."))
    } else {
        let s = fs::read_to_string(&input)
            .with_context(|| format!("reading {}", input.display()))?;
        let base = input.parent().map(Path::to_path_buf).unwrap_or_default();
        (s, base)
    };

    let paper: Paper = cli.paper.parse().map_err(|e: String| anyhow::anyhow!(e))?;
    let today = time::OffsetDateTime::now_local()
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let cfg = Config {
        paper,
        font_size: cli.font_size,
        toc: cli.toc,
        number_headings: cli.number_headings,
        title: cli.title.clone(),
        body_font: cli.body_font.clone(),
        mono_font: cli.mono_font.clone(),
        extra_fonts,
        today: Some((today.year(), today.month() as u8, today.day())),
    };

    // Resolve local images relative to the Markdown file.
    let mut assets: HashMap<String, Vec<u8>> = HashMap::new();
    for rel in md2pdf_core::local_asset_paths(&md) {
        let p = base_dir.join(&rel);
        match fs::read(&p) {
            Ok(bytes) => {
                assets.insert(rel, bytes);
            }
            Err(e) => {
                if !cli.quiet {
                    eprintln!("warning: cannot read image {}: {e}", p.display());
                }
            }
        }
    }

    let out_path = match &cli.output {
        Some(p) => p.clone(),
        None if input.as_os_str() == "-" => PathBuf::from("-"),
        None => input.with_extension(if cli.emit_typst { "typ" } else { "pdf" }),
    };

    if cli.emit_typst {
        let (src, warnings) = md2pdf_core::to_typst(&md, &cfg, &assets);
        print_warnings(&warnings, cli.quiet);
        write_out(&out_path, src.as_bytes())?;
        return Ok(());
    }

    let out = match md2pdf_core::render(&md, &cfg, &assets) {
        Ok(o) => o,
        Err(e) => bail!("{e}"),
    };
    print_warnings(&out.warnings, cli.quiet);
    write_out(&out_path, &out.pdf)?;
    if out_path.as_os_str() != "-" && !cli.quiet {
        eprintln!(
            "wrote {} ({} page{}, {} KiB)",
            out_path.display(),
            out.pages,
            if out.pages == 1 { "" } else { "s" },
            out.pdf.len() / 1024
        );
    }
    Ok(())
}

fn print_warnings(warnings: &[String], quiet: bool) {
    if quiet {
        return;
    }
    for w in warnings {
        eprintln!("warning: {w}");
    }
}

fn write_out(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.as_os_str() == "-" {
        io::stdout().write_all(bytes)?;
        return Ok(());
    }
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

fn load_fonts(dir: &Path) -> Result<Vec<Vec<u8>>> {
    let mut fonts = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading font dir {}", dir.display()))? {
        let p = entry?.path();
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
            fonts.push(fs::read(&p).with_context(|| format!("reading {}", p.display()))?);
        }
    }
    Ok(fonts)
}
