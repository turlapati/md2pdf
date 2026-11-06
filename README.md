# md2pdf

[![ci](https://github.com/turlapati/md2pdf/actions/workflows/ci.yml/badge.svg)](https://github.com/turlapati/md2pdf/actions/workflows/ci.yml)
[![release](https://github.com/turlapati/md2pdf/actions/workflows/release.yml/badge.svg)](https://github.com/turlapati/md2pdf/releases/latest)

GitHub-Flavored Markdown + Mermaid → PDF. One binary (or one WASM module), no
Node, no browser, no LaTeX, no Pandoc, no system fonts, no network.

```
Markdown ──pulldown-cmark──▶ Typst source ──typst (in-memory World)──▶ typst-pdf ──▶ PDF
                 │
          ```mermaid fences ──mermaid-rs-renderer──▶ SVG (virtual file)
```

Supported: headings (with anchors), paragraphs, bold/italic/strike, inline
code, links, images (local), nested bullet/ordered lists, task lists, tables
with alignment, fenced + indented code with syntax highlighting, Mermaid
diagrams (23 types), block quotes, GitHub alerts (`> [!NOTE]` …), footnotes,
definition lists, horizontal rules, `<br>`, YAML front-matter title, optional
table of contents and heading numbering.

See [REVIEW.md](REVIEW.md) for the design review, assumptions and open questions.

## Layout

```
crates/md2pdf-core/   library: Markdown → Typst → PDF (no FS/network/clock; wasm-safe)
crates/md2pdf-cli/    `md2pdf` command-line binary
crates/md2pdf-wasm/   wasm-bindgen wrapper (excluded from the default workspace build)
vscode-extension/     VS Code extension (TypeScript), WASM or native backend
examples/             kitchen-sink.md exercising every feature
.github/workflows/    ci.yml (PRs / master) and release.yml (tags v*)
```

## Download

Prebuilt artifacts are attached to every [GitHub Release](https://github.com/turlapati/md2pdf/releases):

* `md2pdf-<version>.vsix` — VS Code extension (WASM backend, works on any OS)
* `md2pdf-<version>-<platform>[.exe]` — standalone CLI for `win32-x64`, `linux-x64`, `darwin-arm64`, `darwin-x64`

Every CI run (PRs, pushes to `master`, or a manual *Run workflow* on the
[ci workflow](https://github.com/turlapati/md2pdf/actions/workflows/ci.yml)) also uploads
a `vsix` artifact you can download from the run's summary page.

## CI and releases

* **ci.yml** — on every pull request and push to `master`: `cargo test` + `clippy` on
  Linux and Windows, renders `examples/kitchen-sink.md`, builds the WASM module and packages the `.vsix`.
* **release.yml** — on pushing a `v*` tag: checks the tag matches the versions in
  `Cargo.toml` and `vscode-extension/package.json`, runs tests, builds the CLI for four
  platforms and the `.vsix`, and publishes them as a GitHub Release (tags containing `-`,
  e.g. `v0.2.0-rc.1`, become pre-releases).

To cut a release:

```sh
# bump version in Cargo.toml ([workspace.package]) and vscode-extension/package.json
git commit -am "release v0.1.0"
git tag v0.1.0
git push origin master v0.1.0
```

## Build the CLI

```sh
cargo build --release            # → target/release/md2pdf (≈45 MB, fonts embedded)
./target/release/md2pdf examples/kitchen-sink.md --toc
./target/release/md2pdf README.md --paper letter --font-size 11 -o out.pdf
echo '# Hi' | ./target/release/md2pdf - -o -  > hi.pdf     # stdin → stdout
./target/release/md2pdf doc.md --emit-typst                # dump generated Typst for debugging
./target/release/md2pdf --list-fonts                       # embedded font families
```

Cross-compiling for a Windows laptop from Linux/macOS:

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu      # → target/x86_64-pc-windows-gnu/release/md2pdf.exe
```

(or build natively on Windows with the MSVC toolchain; CI does both.)

Offline/reproducible builds: run `cargo vendor` once on a connected machine,
commit `vendor/` and the generated `.cargo/config.toml`.

## Build the WASM module

```sh
cargo install wasm-pack           # once
rustup target add wasm32-unknown-unknown
cd vscode-extension && npm run build:wasm      # → vscode-extension/wasm/md2pdf.{js,wasm,d.ts}
```

## Build and install the VS Code extension

```sh
cd vscode-extension
npm install
npm run build:wasm                 # WASM backend (default), or copy a native binary to bin/<platform>-<arch>/
npm run package                    # → md2pdf-0.1.0.vsix
```

On the target machine: `Ctrl+Shift+P` → **Extensions: Install from VSIX…**.
Then open a Markdown file and use the PDF icon in the editor title bar,
right-click → *Export to PDF* in the explorer, `Ctrl+Shift+Alt+P`, or the
command palette. Settings live under `md2pdf.*` (backend, paper, font size,
TOC, numbering, output directory, open-after-export, export-on-save).

## Library use

```rust
let cfg = md2pdf_core::Config { toc: true, ..Default::default() };
let mut assets = std::collections::HashMap::new();
for p in md2pdf_core::local_asset_paths(&md) {            // local images referenced by the doc
    assets.insert(p.clone(), std::fs::read(&p)?);
}
let out = md2pdf_core::render(&md, &cfg, &assets)?;
std::fs::write("out.pdf", out.pdf)?;
for w in out.warnings { eprintln!("warning: {w}"); }
```

## Known limitations (v0.1)

* LaTeX math is shown as code, not typeset.
* Raw HTML is dropped (except `<br>` and comments).
* Remote images are skipped; relative links to other files are plain text.
* Embedded fonts cover Latin/Greek/Cyrillic only; add CJK/emoji fonts with `--font-dir`.
* Mermaid output is a pure-Rust re-implementation — close to, not identical with, GitHub's.
