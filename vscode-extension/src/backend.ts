/**
 * Compiler backends. Both take the same input and return the same output so
 * the command layer does not care which one is active.
 *
 *  - WasmBackend   : loads wasm/md2pdf.js (built by wasm-pack) in-process.
 *  - NativeBackend : spawns the md2pdf executable with --output -.
 */
import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';
import { execFile } from 'child_process';

export interface RenderOptions {
  paper: 'a4' | 'letter';
  fontSize: number;
  toc: boolean;
  numberHeadings: boolean;
  title?: string;
  remoteImages: 'altText' | 'embed';
}

export interface RenderInput {
  markdown: string;
  /** Directory used to resolve relative image paths. */
  baseDir: string;
  options: RenderOptions;
}

export interface RenderOutput {
  pdf: Uint8Array;
  warnings: string[];
  pages?: number;
}

export interface Backend {
  readonly name: string;
  render(input: RenderInput): Promise<RenderOutput>;
  toTypst(input: RenderInput): Promise<{ source: string; warnings: string[] }>;
}

// ---------------------------------------------------------------------------
// WASM backend
// ---------------------------------------------------------------------------

interface WasmModule {
  local_asset_paths(md: string): string[];
  remote_image_urls(md: string): string[];
  render(md: string, assets: Record<string, Uint8Array>, options: unknown): {
    pdf: Uint8Array;
    warnings: string[];
    pages: number;
    free(): void;
  };
  to_typst(md: string, assets: Record<string, Uint8Array>, options: unknown): string;
}

const MAX_REMOTE_IMAGE_BYTES = 10 * 1024 * 1024;

async function fetchImage(url: string): Promise<Uint8Array> {
  const res = await fetch(url, { signal: AbortSignal.timeout(15_000), redirect: 'follow' });
  if (!res.ok) {
    throw new Error(`HTTP ${res.status} ${res.statusText}`);
  }
  const body = new Uint8Array(await res.arrayBuffer());
  if (body.byteLength > MAX_REMOTE_IMAGE_BYTES) {
    throw new Error(`larger than ${MAX_REMOTE_IMAGE_BYTES / (1024 * 1024)} MB`);
  }
  return body;
}

function todayTuple(): [number, number, number] {
  const d = new Date();
  return [d.getFullYear(), d.getMonth() + 1, d.getDate()];
}

export class WasmBackend implements Backend {
  readonly name = 'wasm';
  private mod: WasmModule | undefined;

  constructor(private readonly extensionPath: string) {}

  private load(): WasmModule {
    if (!this.mod) {
      const entry = path.join(this.extensionPath, 'wasm', 'md2pdf.js');
      if (!fs.existsSync(entry)) {
        throw new Error(
          `WASM module not found at ${entry}. Build it with "npm run build:wasm" (requires wasm-pack) or switch md2pdf.backend to "native".`,
        );
      }
      // eslint-disable-next-line @typescript-eslint/no-require-imports
      this.mod = require(entry) as WasmModule;
    }
    return this.mod;
  }

  private async collectAssets(
    md: string,
    baseDir: string,
    options: RenderOptions,
  ): Promise<{ assets: Record<string, Uint8Array>; warnings: string[] }> {
    const mod = this.load();
    const assets: Record<string, Uint8Array> = {};
    const warnings: string[] = [];
    if (options.remoteImages === 'embed') {
      await Promise.all(
        mod.remote_image_urls(md).map(async (url) => {
          try {
            assets[url] = await fetchImage(url);
          } catch (err) {
            const reason = err instanceof Error ? err.message : String(err);
            warnings.push(`cannot fetch remote image ${url} (showing alt text): ${reason}`);
          }
        }),
      );
    }
    for (const rel of mod.local_asset_paths(md)) {
      const abs = path.resolve(baseDir, rel);
      // Guard against ../../ escaping the workspace folder.
      const ws = vscode.workspace.getWorkspaceFolder(vscode.Uri.file(abs));
      if (!ws && vscode.workspace.workspaceFolders?.length) {
        continue;
      }
      try {
        assets[rel] = await vscode.workspace.fs.readFile(vscode.Uri.file(abs));
      } catch {
        // Missing files are reported by the core as warnings.
      }
    }
    return { assets, warnings };
  }

  private wasmOptions(o: RenderOptions) {
    return {
      paper: o.paper,
      fontSize: o.fontSize,
      toc: o.toc,
      numberHeadings: o.numberHeadings,
      title: o.title,
      today: todayTuple(),
      remoteImages: o.remoteImages,
    };
  }

  async render(input: RenderInput): Promise<RenderOutput> {
    const mod = this.load();
    const { assets, warnings } = await this.collectAssets(input.markdown, input.baseDir, input.options);
    const result = mod.render(input.markdown, assets, this.wasmOptions(input.options));
    try {
      return { pdf: result.pdf, warnings: warnings.concat(result.warnings), pages: result.pages };
    } finally {
      result.free();
    }
  }

  async toTypst(input: RenderInput) {
    const mod = this.load();
    const { assets, warnings } = await this.collectAssets(input.markdown, input.baseDir, input.options);
    return { source: mod.to_typst(input.markdown, assets, this.wasmOptions(input.options)), warnings };
  }
}

// ---------------------------------------------------------------------------
// Native backend
// ---------------------------------------------------------------------------

export class NativeBackend implements Backend {
  readonly name = 'native';

  constructor(private readonly extensionPath: string, private readonly configuredPath: string) {}

  private binary(): string {
    if (this.configuredPath.trim()) {
      return this.configuredPath;
    }
    const exe = process.platform === 'win32' ? 'md2pdf.exe' : 'md2pdf';
    const bundled = path.join(this.extensionPath, 'bin', `${process.platform}-${process.arch}`, exe);
    if (!fs.existsSync(bundled)) {
      throw new Error(
        `Native md2pdf binary not found at ${bundled}. Set md2pdf.nativeBinaryPath or switch md2pdf.backend to "wasm".`,
      );
    }
    return bundled;
  }

  private args(o: RenderOptions, extra: string[]): string[] {
    const a = [
      '-',
      '--output',
      '-',
      '--paper',
      o.paper,
      '--font-size',
      String(o.fontSize),
      '--remote-images',
      o.remoteImages === 'embed' ? 'embed' : 'alt-text',
    ];
    if (o.toc) {
      a.push('--toc');
    }
    if (o.numberHeadings) {
      a.push('--number-headings');
    }
    if (o.title) {
      a.push('--title', o.title);
    }
    return a.concat(extra);
  }

  private run(input: RenderInput, extra: string[]): Promise<{ stdout: Buffer; stderr: string }> {
    const bin = this.binary();
    return new Promise((resolve, reject) => {
      const child = execFile(
        bin,
        this.args(input.options, extra),
        { cwd: input.baseDir, encoding: 'buffer', maxBuffer: 256 * 1024 * 1024, windowsHide: true },
        (err, stdout, stderr) => {
          const errText = Buffer.from(stderr).toString('utf8');
          if (err) {
            reject(new Error(errText || err.message));
          } else {
            resolve({ stdout: Buffer.from(stdout), stderr: errText });
          }
        },
      );
      child.stdin?.end(input.markdown, 'utf8');
    });
  }

  private warningsFrom(stderr: string): string[] {
    return stderr
      .split(/\r?\n/)
      .filter((l) => l.startsWith('warning:'))
      .map((l) => l.replace(/^warning:\s*/, ''));
  }

  async render(input: RenderInput): Promise<RenderOutput> {
    const { stdout, stderr } = await this.run(input, []);
    return { pdf: new Uint8Array(stdout), warnings: this.warningsFrom(stderr) };
  }

  async toTypst(input: RenderInput) {
    const { stdout, stderr } = await this.run(input, ['--emit-typst']);
    return { source: stdout.toString('utf8'), warnings: this.warningsFrom(stderr) };
  }
}
