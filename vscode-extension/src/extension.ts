import * as vscode from 'vscode';
import * as path from 'path';
import { Backend, NativeBackend, RenderInput, RenderOptions, WasmBackend } from './backend';

let output: vscode.OutputChannel;

export function activate(context: vscode.ExtensionContext) {
  output = vscode.window.createOutputChannel('md2pdf');
  context.subscriptions.push(output);

  context.subscriptions.push(
    vscode.commands.registerCommand('md2pdf.export', (uri?: vscode.Uri) => exportCommand(context, uri, 'pdf')),
    vscode.commands.registerCommand('md2pdf.exportTypst', (uri?: vscode.Uri) => exportCommand(context, uri, 'typst')),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.languageId === 'markdown' && config().get<boolean>('exportOnSave', false)) {
        void exportDocument(context, doc, 'pdf', { silent: true });
      }
    }),
  );
}

export function deactivate() {}

function config() {
  return vscode.workspace.getConfiguration('md2pdf');
}

function makeBackend(context: vscode.ExtensionContext): Backend {
  const kind = config().get<string>('backend', 'wasm');
  if (kind === 'native') {
    return new NativeBackend(context.extensionPath, config().get<string>('nativeBinaryPath', ''));
  }
  return new WasmBackend(context.extensionPath);
}

function renderOptions(): RenderOptions {
  const c = config();
  return {
    paper: c.get<'a4' | 'letter'>('paper', 'a4'),
    fontSize: c.get<number>('fontSize', 10.5),
    toc: c.get<boolean>('toc', false),
    numberHeadings: c.get<boolean>('numberHeadings', false),
    remoteImages: c.get<'altText' | 'embed'>('remoteImages', 'altText'),
  };
}

async function exportCommand(context: vscode.ExtensionContext, uri: vscode.Uri | undefined, kind: 'pdf' | 'typst') {
  let doc: vscode.TextDocument | undefined;
  if (uri) {
    doc = await vscode.workspace.openTextDocument(uri);
  } else {
    doc = vscode.window.activeTextEditor?.document;
  }
  if (!doc || doc.languageId !== 'markdown') {
    void vscode.window.showErrorMessage('md2pdf: open a Markdown file first.');
    return;
  }
  await exportDocument(context, doc, kind, { silent: false });
}

async function exportDocument(
  context: vscode.ExtensionContext,
  doc: vscode.TextDocument,
  kind: 'pdf' | 'typst',
  opts: { silent: boolean },
) {
  if (doc.isUntitled) {
    void vscode.window.showErrorMessage('md2pdf: save the file first so relative image paths and the output location are known.');
    return;
  }

  const srcPath = doc.uri.fsPath;
  const baseDir = path.dirname(srcPath);
  const outDirSetting = config().get<string>('outputDirectory', '').trim();
  const outDir = outDirSetting ? path.resolve(baseDir, outDirSetting) : baseDir;
  const stem = path.basename(srcPath).replace(/\.(md|markdown)$/i, '');
  const outPath = path.join(outDir, `${stem}.${kind === 'pdf' ? 'pdf' : 'typ'}`);

  const input: RenderInput = {
    markdown: doc.getText(), // unsaved edits are included on purpose
    baseDir,
    options: renderOptions(),
  };

  const backend = makeBackend(context);
  const started = Date.now();

  try {
    await vscode.window.withProgress(
      { location: vscode.ProgressLocation.Window, title: `md2pdf: exporting ${path.basename(srcPath)}…` },
      async () => {
        await vscode.workspace.fs.createDirectory(vscode.Uri.file(outDir));
        let warnings: string[];
        if (kind === 'pdf') {
          const result = await backend.render(input);
          await vscode.workspace.fs.writeFile(vscode.Uri.file(outPath), result.pdf);
          warnings = result.warnings;
        } else {
          const result = await backend.toTypst(input);
          await vscode.workspace.fs.writeFile(vscode.Uri.file(outPath), Buffer.from(result.source, 'utf8'));
          warnings = result.warnings;
        }

        const ms = Date.now() - started;
        output.appendLine(`[${new Date().toISOString()}] ${backend.name}: ${srcPath} → ${outPath} (${ms} ms)`);
        for (const w of warnings) {
          output.appendLine(`  warning: ${w}`);
        }

        if (!opts.silent) {
          const msg = `md2pdf: wrote ${path.basename(outPath)}${warnings.length ? ` (${warnings.length} warning${warnings.length === 1 ? '' : 's'})` : ''}`;
          const actions = warnings.length ? ['Show warnings'] : [];
          const choice = await vscode.window.showInformationMessage(msg, ...actions);
          if (choice === 'Show warnings') {
            output.show(true);
          }
        }

        if (kind === 'pdf' && config().get<boolean>('openAfterExport', true) && !opts.silent) {
          await vscode.env.openExternal(vscode.Uri.file(outPath));
        }
      },
    );
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    output.appendLine(`[${new Date().toISOString()}] ERROR ${srcPath}\n${message}`);
    output.show(true);
    void vscode.window.showErrorMessage(`md2pdf: export failed — see the md2pdf output channel.`);
  }
}
