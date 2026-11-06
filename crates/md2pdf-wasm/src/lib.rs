//! WASM entry points. Build with:
//!
//! ```sh
//! wasm-pack build crates/md2pdf-wasm --target nodejs --release
//! ```
//!
//! The JS side must:
//! 1. call `local_asset_paths(md)` to learn which local images the document
//!    references,
//! 2. read those files itself (the WASM module has no filesystem), and with
//!    `remoteImages: "embed"` download the URLs from `remote_image_urls(md)`,
//! 3. call `render(md, assets, options)` where `assets` is a plain object
//!    `{ "<path as written>": Uint8Array }`.

use std::collections::HashMap;

use serde::Deserialize;
use wasm_bindgen::prelude::*;

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Options {
    paper: Option<String>,
    font_size: Option<f32>,
    toc: bool,
    number_headings: bool,
    title: Option<String>,
    body_font: Option<String>,
    mono_font: Option<String>,
    /// [year, month, day] from the host (no clock in wasm32-unknown-unknown).
    today: Option<(i32, u8, u8)>,
    /// "altText" (default) or "embed".
    remote_images: Option<String>,
}

#[wasm_bindgen]
pub fn local_asset_paths(md: &str) -> Vec<String> {
    md2pdf_core::local_asset_paths(md)
}

/// http(s) image URLs the host should download when `remoteImages: "embed"`;
/// pass the bytes in `assets` keyed by URL.
#[wasm_bindgen]
pub fn remote_image_urls(md: &str) -> Vec<String> {
    md2pdf_core::remote_image_urls(md)
}

#[wasm_bindgen]
pub struct RenderResult {
    pdf: Vec<u8>,
    warnings: Vec<String>,
    pages: usize,
}

#[wasm_bindgen]
impl RenderResult {
    #[wasm_bindgen(getter)]
    pub fn pdf(&self) -> Vec<u8> {
        self.pdf.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn warnings(&self) -> Vec<String> {
        self.warnings.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn pages(&self) -> usize {
        self.pages
    }
}

fn build_config(options: JsValue) -> Result<md2pdf_core::Config, JsValue> {
    let o: Options = if options.is_undefined() || options.is_null() {
        Options::default()
    } else {
        serde_wasm_bindgen::from_value(options).map_err(|e| JsValue::from_str(&e.to_string()))?
    };
    let mut cfg = md2pdf_core::Config::default();
    if let Some(p) = o.paper {
        cfg.paper = p.parse().map_err(|e: String| JsValue::from_str(&e))?;
    }
    if let Some(s) = o.font_size {
        cfg.font_size = s;
    }
    cfg.toc = o.toc;
    cfg.number_headings = o.number_headings;
    cfg.title = o.title;
    if let Some(f) = o.body_font {
        cfg.body_font = f;
    }
    if let Some(f) = o.mono_font {
        cfg.mono_font = f;
    }
    cfg.today = o.today;
    if let Some(r) = o.remote_images {
        cfg.remote_images = r.parse().map_err(|e: String| JsValue::from_str(&e))?;
    }
    Ok(cfg)
}

fn assets_from_js(assets: JsValue) -> Result<HashMap<String, Vec<u8>>, JsValue> {
    let mut map = HashMap::new();
    if assets.is_undefined() || assets.is_null() {
        return Ok(map);
    }
    let obj = js_sys::Object::from(assets);
    for key in js_sys::Object::keys(&obj).iter() {
        let k = key.as_string().unwrap_or_default();
        let v = js_sys::Reflect::get(&obj, &key)?;
        let bytes = js_sys::Uint8Array::new(&v).to_vec();
        map.insert(k, bytes);
    }
    Ok(map)
}

#[wasm_bindgen]
pub fn render(md: &str, assets: JsValue, options: JsValue) -> Result<RenderResult, JsValue> {
    let cfg = build_config(options)?;
    let assets = assets_from_js(assets)?;
    let out = md2pdf_core::render(md, &cfg, &assets).map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(RenderResult { pdf: out.pdf, warnings: out.warnings, pages: out.pages })
}

#[wasm_bindgen]
pub fn to_typst(md: &str, assets: JsValue, options: JsValue) -> Result<String, JsValue> {
    let cfg = build_config(options)?;
    let assets = assets_from_js(assets)?;
    Ok(md2pdf_core::to_typst(md, &cfg, &assets).0)
}
