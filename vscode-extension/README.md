# md2pdf

[![ci](https://github.com/turlapati/md2pdf/actions/workflows/ci.yml/badge.svg)](https://github.com/turlapati/md2pdf/actions/workflows/ci.yml)
[![release](https://github.com/turlapati/md2pdf/actions/workflows/release.yml/badge.svg)](https://github.com/turlapati/md2pdf/releases/latest)

Export GitHub-Flavored Markdown to PDF from the editor. Math and Mermaid
diagrams are included. The compiler is built in, so the export needs no Node,
browser, LaTeX, Pandoc, or system fonts, and it works fully offline.

Markdown is parsed to Typst, LaTeX math is converted with mitex, and Mermaid
diagrams are drawn to SVG. A Typst compiler then writes the PDF.

## Features

* Headings with clickable in-document links, optional table of contents and heading numbering
* Bold, italic, strikethrough, inline code, links and local images (PNG, JPEG, GIF, SVG, WebP)
* Nested bullet and numbered lists, task lists, tables with column alignment
* Fenced and indented code blocks with syntax highlighting
* LaTeX math (`$…$`, `$$…$$`, and fenced `math` / `latex` / `tex` blocks)
* Mermaid diagrams (23 diagram types)
* Block quotes, GitHub alerts (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`)
* Footnotes, definition lists, horizontal rules, `<br>` / `<sub>` / `<sup>` / `<kbd>`
* Document title from YAML front matter (`title:`) or the first heading
* A4 or US Letter paper

## Installation

In Cursor and VSCodium, open the Extensions view and search for **md2pdf**.

In VS Code, download `md2pdf-<version>.vsix` from the [latest release](https://github.com/turlapati/md2pdf/releases/latest), press `Ctrl+Shift+P` (`Cmd+Shift+P` on macOS), run **Extensions: Install from VSIX…**, and pick that file.

## Use

Open a Markdown file, then do any of the following:

* Click the PDF icon in the editor title bar
* Press `Ctrl+Shift+Alt+P` (`Cmd+Shift+Alt+P` on macOS)
* Right-click a `.md` file in the Explorer and choose **Export to PDF**
* Run **md2pdf: Export to PDF** from the Command Palette

The PDF is written next to the Markdown file and opened in your default viewer.
Unsaved edits are included. Any warnings (for example, a missing image) are
listed in the **md2pdf** output panel.

### Settings

| Setting | Default | Description |
|---|---|---|
| `md2pdf.paper` | `a4` | Paper size: `a4` or `letter` |
| `md2pdf.fontSize` | `10.5` | Body font size in points |
| `md2pdf.toc` | `false` | Insert a table of contents |
| `md2pdf.numberHeadings` | `false` | Number headings (1, 1.1, …) |
| `md2pdf.remoteImages` | `altText` | How `http(s)` images are handled — see [Remote images](#remote-images) |
| `md2pdf.outputDirectory` | *(empty)* | Folder for PDFs, relative to the Markdown file; empty means next to it |
| `md2pdf.openAfterExport` | `true` | Open the PDF after exporting |
| `md2pdf.exportOnSave` | `false` | Re-export every time a Markdown file is saved |
| `md2pdf.backend` | `wasm` | `wasm` runs inside the editor; `native` runs the command-line tool (faster for very large documents) |
| `md2pdf.nativeBinaryPath` | *(empty)* | Path to the command-line tool when `backend` is `native` |

## Remote images

Images with an `http://` or `https://` address, such as status badges, are
handled according to `md2pdf.remoteImages`:

* **`altText` (default)** — nothing is downloaded. The image's alt text is shown as a link to the image. For a linked image such as `[![build](badge.svg)](https://ci.example.com)`, the alt text links to the outer address.
* **`embed`** — the image is downloaded (15-second timeout, 10 MB limit) and placed in the PDF. Images inside a link are drawn inline at text height, the way badges appear on GitHub. If a download fails, the alt text is used and a warning is shown.

## Known limitations

* Raw HTML is ignored, except `<br>`, `<sub>`, `<sup>`, `<kbd>`, and comments.
* Links to other local files appear as plain text, since they can't be followed inside a PDF.
* The built-in fonts cover Latin, Greek and Cyrillic. Chinese, Japanese, Korean, and emoji need an extra font, which the command-line tool can load.
* Mermaid diagrams are drawn by a separate renderer, so they look close to, but not identical to, GitHub's.

## Command-line tool

The same compiler ships as a single binary. Usage and build instructions are in the [project README](https://github.com/turlapati/md2pdf/blob/master/README.md) and [CONTRIBUTING.md](https://github.com/turlapati/md2pdf/blob/master/CONTRIBUTING.md).

## License

[MIT](https://github.com/turlapati/md2pdf/blob/master/LICENSE)
