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
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
