// Renders every example document through the built WASM module. A Rust panic
// inside WASM surfaces as "RuntimeError: unreachable", which this catches.
const fs = require('fs');
const path = require('path');

const mod = require(path.join(__dirname, '..', 'wasm', 'md2pdf.js'));
const examples = path.join(__dirname, '..', '..', 'examples');

function markdownFiles(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) return markdownFiles(p);
    return e.name.endsWith('.md') ? [p] : [];
  });
}

let failed = 0;
for (const file of markdownFiles(examples)) {
  const md = fs.readFileSync(file, 'utf8');
  const assets = {};
  for (const rel of mod.local_asset_paths(md)) {
    const abs = path.resolve(path.dirname(file), rel);
    if (fs.existsSync(abs)) assets[rel] = fs.readFileSync(abs);
  }
  const name = path.relative(examples, file);
  try {
    const out = mod.render(md, assets, { today: [2026, 1, 1] });
    console.log(`ok   ${name} (${out.pages} page${out.pages === 1 ? '' : 's'})`);
    out.free();
  } catch (err) {
    failed++;
    console.error(`FAIL ${name}: ${err && err.message}`);
  }
}
process.exit(failed ? 1 : 0);
