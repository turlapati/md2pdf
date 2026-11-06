# Contributing

## Repository layout

```
crates/md2pdf-core/   library: Markdown → Typst → PDF (no filesystem/network/clock; wasm-safe)
crates/md2pdf-cli/    md2pdf command-line binary
crates/md2pdf-wasm/   wasm-bindgen wrapper (excluded from the default workspace build)
vscode-extension/     VS Code extension (TypeScript), WASM or native backend
examples/             kitchen-sink.md exercising every feature, plus edge cases
third_party/          mermaid-rs-renderer 0.3.1 patched for wasm32 (used by md2pdf-wasm only)
.github/workflows/    ci.yml (PRs and master) and release.yml (v* tags)
```

## Build the CLI

```sh
cargo build --release            # → target/release/md2pdf (fonts embedded)
cargo test --workspace
./target/release/md2pdf examples/kitchen-sink.md --toc
```

## Build the VS Code extension

Requires [wasm-pack](https://rustwasm.github.io/wasm-pack/) and the
`wasm32-unknown-unknown` target:

```sh
cargo install wasm-pack
rustup target add wasm32-unknown-unknown

cd vscode-extension
npm install
npm run build:wasm               # → vscode-extension/wasm/
npm run test:wasm                # render every file in examples/ through the WASM module
npm run package                  # → md2pdf-<version>.vsix
```

The WASM build uses a patched copy of `mermaid-rs-renderer` in
`third_party/` (via `[patch.crates-io]` in `crates/md2pdf-wasm/Cargo.toml`):
upstream calls `std::time::Instant::now()`, which aborts on
`wasm32-unknown-unknown`. When upgrading `mermaid-rs-renderer`, re-copy its
`src/` and reapply `src/time_compat.rs` plus the `use crate::time_compat::Instant`
imports, then run `npm run test:wasm`.

To bundle a native binary for the `native` backend, copy it to
`vscode-extension/bin/<platform>-<arch>/md2pdf[.exe]` (for example
`bin/win32-x64/md2pdf.exe`) before packaging.

## Library use

```rust
let cfg = md2pdf_core::Config { toc: true, ..Default::default() };
let mut assets = std::collections::HashMap::new();
for p in md2pdf_core::local_asset_paths(&md) {
    assets.insert(p.clone(), std::fs::read(&p)?);
}
let out = md2pdf_core::render(&md, &cfg, &assets)?;
std::fs::write("out.pdf", out.pdf)?;
for w in out.warnings { eprintln!("warning: {w}"); }
```

## Golden visual tests

Selected examples are rendered to PDF with a fixed `--date`, rasterized with
`pdftoppm`, and compared to PNGs in `testdata/golden/`. Requires
[Poppler](https://poppler.freedesktop.org/) (`pdftoppm`).

```sh
cargo build -p md2pdf
bash scripts/golden-test.sh                 # compare
UPDATE_GOLDEN=1 bash scripts/golden-test.sh  # regenerate references
```

Regenerate on Linux (same as CI) when fonts or layout change, so the committed
PNGs match `ubuntu-latest`.

## CI

`ci.yml` runs on every pull request and push to `master`: `cargo test` and
`clippy` on Linux and Windows, golden visual tests (Ubuntu + Poppler), renders
`examples/kitchen-sink.md`, builds the WASM module and packages the `.vsix`.
Each run uploads a `vsix` artifact; it can also be started manually from the
Actions tab (*Run workflow*).

## Releasing

1. Bump the version in `Cargo.toml` (`[workspace.package]`) and
   `vscode-extension/package.json`.
2. Commit, tag and push:

   ```sh
   git commit -am "release v0.2.0"
   git tag v0.2.0
   git push origin master v0.2.0
   ```

`release.yml` checks that the tag matches both versions, runs the tests, builds
the CLI for Windows x64, Linux x64 and macOS (arm64 and x64) plus the `.vsix`,
and publishes them as a GitHub Release. Tags containing `-` (e.g. `v0.2.0-rc.1`)
become pre-releases.
