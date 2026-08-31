// Client half of the LSP wiring — `kolom-lsp` (crates/kolom-lsp) is the
// server, a plain Rust binary speaking Content-Length-framed JSON-RPC over
// stdio. This file's only job is finding that binary and handing it to
// vscode-languageclient; there is no diagnostic logic here at all, so a
// change to what Kolom reports as an error never needs a change here.
import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";
import { KolomConsoleViewProvider } from "./consoleView";

let client: LanguageClient | undefined;

function exeName(base: string): string {
  return process.platform === "win32" ? `${base}.exe` : base;
}

/** Searches PATH for `base` the same way a shell would, returning the first match. */
function findOnPath(base: string): string | undefined {
  const pathEnv = process.env.PATH ?? process.env.Path ?? "";
  const name = exeName(base);
  for (const dir of pathEnv.split(path.delimiter)) {
    if (!dir) {
      continue;
    }
    const candidate = path.join(dir, name);
    try {
      if (fs.statSync(candidate).isFile()) {
        return candidate;
      }
    } catch {
      // Not present in this PATH entry — keep looking.
    }
  }
  return undefined;
}

/**
 * Resolution order: explicit `kolom.lspPath` setting, then a `kolom-lsp`
 * living next to whichever `kolom` is on PATH (how `scripts/make-sysroot.sh`
 * ships them — same directory), then a bare name for the OS's own PATH
 * lookup to resolve at spawn time.
 */
function resolveLspPath(): string {
  const configured = vscode.workspace
    .getConfiguration("kolom")
    .get<string>("lspPath", "");
  if (configured && configured.trim().length > 0) {
    return configured.trim();
  }
  const kolomExe = findOnPath("kolom");
  if (kolomExe) {
    const sibling = path.join(path.dirname(kolomExe), exeName("kolom-lsp"));
    if (fs.existsSync(sibling)) {
      return sibling;
    }
  }
  return exeName("kolom-lsp");
}

/** Same resolution order as `resolveLspPath`, but for the `kolom` CLI itself. */
function resolveKolomCliPath(): string {
  const configured = vscode.workspace
    .getConfiguration("kolom")
    .get<string>("cliPath", "");
  if (configured && configured.trim().length > 0) {
    return configured.trim();
  }
  const kolomExe = findOnPath("kolom");
  if (kolomExe) {
    return kolomExe;
  }
  return exeName("kolom");
}

/** Where a typed `kolom` command (not tied to a specific file) should run. */
function resolveCommandCwd(): string {
  const editor = vscode.window.activeTextEditor;
  if (editor && editor.document.languageId === "kolom") {
    return path.dirname(editor.document.fileName);
  }
  const folder = vscode.workspace.workspaceFolders?.[0];
  return folder ? folder.uri.fsPath : process.cwd();
}

/**
 * Backs both the Command Palette entry and the console panel's own
 * button/Enter. `commandLine` is the panel input box's raw text: empty means
 * "run the active `.ক` file" (also what the Palette command always passes);
 * anything else is a full `kolom` command line — `kolom` already accepts
 * Bangla subcommand names directly (`ইনস্টল`, `যোগ`, `মুছো`, ...), so this
 * just splits on whitespace and hands the words straight through, no
 * translation needed.
 */
async function runCommandInConsole(provider: KolomConsoleViewProvider, commandLine: string): Promise<void> {
  const kolomPath = resolveKolomCliPath();
  const trimmed = commandLine.trim();
  if (trimmed.length === 0) {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== "kolom") {
      void vscode.window.showErrorMessage(
        "কলম: প্রথমে একটি .ক ফাইল খুলুন, অথবা কনসোলের বক্সে একটি kolom কমান্ড লিখুন।",
      );
      return;
    }
    if (editor.document.isDirty) {
      await editor.document.save();
    }
    const filePath = editor.document.fileName;
    await provider.run(kolomPath, ["run", filePath], path.dirname(filePath), path.basename(filePath));
    return;
  }

  const args = trimmed.split(/\s+/);
  await provider.run(kolomPath, args, resolveCommandCwd(), `kolom ${trimmed}`);
}

export function activate(context: vscode.ExtensionContext): void {
  const lspPath = resolveLspPath();

  const serverOptions: ServerOptions = {
    command: lspPath,
    transport: TransportKind.stdio,
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "kolom" }],
  };

  client = new LanguageClient(
    "kolom-lsp",
    "Kolom Language Server",
    serverOptions,
    clientOptions,
  );

  client.start().then(undefined, (err: unknown) => {
    const detail = err instanceof Error ? err.message : String(err);
    void vscode.window.showErrorMessage(
      `কলম: '${lspPath}' চালানো যায়নি — লাইভ ত্রুটি বন্ধ থাকবে (সিনট্যাক্স হাইলাইটিং তবু কাজ করবে)। ` +
        `ইনস্টল যাচাই করুন (docs/getting-started.md), অথবা "kolom.lspPath" সেটিং দিয়ে সরাসরি পথ দিন। (${detail})`,
    );
  });

  context.subscriptions.push({
    dispose: () => {
      void client?.stop();
    },
  });

  const consoleProvider = new KolomConsoleViewProvider((commandLine) => {
    void runCommandInConsole(consoleProvider, commandLine);
  });
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(KolomConsoleViewProvider.panelViewId, consoleProvider),
  );
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(KolomConsoleViewProvider.sidebarViewId, consoleProvider),
  );
  context.subscriptions.push(
    vscode.commands.registerCommand("kolom.runInConsole", () => runCommandInConsole(consoleProvider, "")),
  );
  context.subscriptions.push({ dispose: () => consoleProvider.dispose() });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
