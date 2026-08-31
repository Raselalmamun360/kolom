// A panel tab beside Terminal/Output/Problems that runs a `.ক` file and shows
// its output as plain HTML text instead of feeding a terminal emulator.
// That's the whole point: every terminal renders text into a fixed
// monospace character-cell grid, which cannot represent Bengali's reordering
// vowel signs (কার — ে/ো/ৌ, which must appear *before* the consonant they
// logically follow). A webview is a real Chromium page, so plain text in it
// shapes correctly automatically — no shaping library, no native code needed
// (contrast with `kolom console`, the native Win32 window in kolom-runtime,
// which has to reuse Uniscribe explicitly to get the same correctness).
import * as cp from "child_process";
import * as vscode from "vscode";

function nonce(): string {
  let text = "";
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  for (let i = 0; i < 32; i++) {
    text += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return text;
}

/**
 * One provider instance is registered under two different view ids — a
 * bottom-panel tab (beside Terminal) and an Activity Bar sidebar — so both
 * entry points show the same live output. `views` therefore holds however
 * many of the two are currently resolved (0, 1, or both); `post` broadcasts
 * to all of them, and `outputSoFar`/`statusSoFar` let a view that resolves
 * *after* a run has already started replay what it missed instead of
 * opening blank.
 */
export class KolomConsoleViewProvider implements vscode.WebviewViewProvider {
  public static readonly panelViewId = "kolom.consoleView";
  public static readonly panelContainerId = "kolomConsolePanel";
  public static readonly sidebarViewId = "kolom.consoleViewSidebar";
  public static readonly sidebarContainerId = "kolomSidebar";

  private readonly views = new Set<vscode.WebviewView>();
  private proc: cp.ChildProcess | undefined;
  private outputSoFar = "";
  private statusSoFar = "";

  /**
   * @param onRunRequested Invoked with the box's raw text when the panel's
   * own button/Enter is pressed while no program is running — chat-style,
   * the same control doubles as "run" (idle) and "send" (once a program is
   * live). An empty string means "run the active `.ক` file" (the common
   * case); non-empty text is a full `kolom` command line — e.g. `ইনস্টল` or
   * `যোগ foo https://...` — since `kolom` already accepts Bangla subcommand
   * names directly (see kolom-cli's HELP text), no translation layer is
   * needed here, just argv splitting. Resolving either case needs
   * `vscode.window.activeTextEditor` and the `kolom` CLI path, both of
   * which live in extension.ts, hence the callback rather than doing it
   * here.
   */
  constructor(private readonly onRunRequested: (commandLine: string) => void) {}

  resolveWebviewView(webviewView: vscode.WebviewView): void {
    this.views.add(webviewView);
    webviewView.webview.options = { enableScripts: true };
    webviewView.webview.html = this.renderHtml(webviewView.webview.cspSource);
    // Posting state right here would race the webview's page load: the
    // iframe's script (and its message listener) may not have run yet, and
    // a message posted before that listener attaches is dropped, not
    // queued. So instead the page tells us when it's actually listening
    // (the "ready" message below), and only then do we replay state —
    // to that one view, since a second view resolving later must not
    // re-replay into views that are already caught up.
    webviewView.webview.onDidReceiveMessage((msg: { type: string; text?: string }) => {
      if (msg.type === "input" && this.proc?.stdin?.writable) {
        this.proc.stdin.write(`${msg.text ?? ""}\n`);
      } else if (msg.type === "run") {
        this.onRunRequested(msg.text ?? "");
      } else if (msg.type === "ready") {
        if (this.outputSoFar) {
          void webviewView.webview.postMessage({ type: "output", text: this.outputSoFar });
        }
        if (this.statusSoFar) {
          void webviewView.webview.postMessage({ type: "status", text: this.statusSoFar });
        }
        void webviewView.webview.postMessage({ type: "running", running: this.proc !== undefined });
      }
    });
    webviewView.onDidDispose(() => {
      this.views.delete(webviewView);
    });
  }

  /** Reveals the given container's view, resolving it first if not yet shown. */
  private async reveal(containerId: string, viewId: string): Promise<void> {
    const already = [...this.views].some((v) => v.viewType === viewId);
    if (!already) {
      await vscode.commands.executeCommand(`workbench.view.extension.${containerId}`);
      // resolveWebviewView runs synchronously off that command in practice,
      // but give it one tick of slack rather than assume timing.
      for (let i = 0; i < 20 && ![...this.views].some((v) => v.viewType === viewId); i++) {
        await new Promise((r) => setTimeout(r, 25));
      }
    }
    for (const v of this.views) {
      if (v.viewType === viewId) {
        v.show(true);
      }
    }
  }

  /**
   * Runs `kolomPath` with `args` as-is — the general form behind both "run
   * the active file" (`args = ["run", filePath]`) and typing any other
   * `kolom` command line into the panel (`args` = that line, whitespace-
   * split). `label` is what the status line calls it while it runs.
   */
  async run(kolomPath: string, args: string[], cwd: string, label: string): Promise<void> {
    // The bottom panel is where this feature originally landed, so a run
    // always surfaces there; the sidebar (if the user also has it open, or
    // opens it later) mirrors the same live feed via the replay above.
    await this.reveal(KolomConsoleViewProvider.panelContainerId, KolomConsoleViewProvider.panelViewId);
    this.killCurrent();
    this.outputSoFar = "";
    this.post({ type: "clear" });
    this.setStatus(`▶ চলছে: ${label}`);

    const child = cp.spawn(kolomPath, args, { cwd });
    this.proc = child;
    this.post({ type: "running", running: true });

    child.stdout?.setEncoding("utf8");
    child.stderr?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => this.appendOutput(chunk));
    child.stderr?.on("data", (chunk: string) => this.appendOutput(chunk));

    child.on("error", (err) => {
      this.setStatus(`ত্রুটি: '${kolomPath}' চালানো যায়নি — ${err.message}`);
      if (this.proc === child) {
        this.proc = undefined;
      }
      this.post({ type: "running", running: false });
    });
    child.on("close", (code) => {
      if (this.proc === child) {
        this.proc = undefined;
      }
      this.setStatus(code === 0 ? "[প্রোগ্রাম শেষ হয়েছে]" : `[প্রোগ্রাম শেষ হয়েছে — exit code ${code}]`);
      this.post({ type: "running", running: false });
    });
  }

  private killCurrent(): void {
    if (this.proc && !this.proc.killed) {
      this.proc.kill();
    }
    this.proc = undefined;
  }

  dispose(): void {
    this.killCurrent();
  }

  private appendOutput(text: string): void {
    this.outputSoFar += text;
    this.post({ type: "output", text });
  }

  private setStatus(text: string): void {
    this.statusSoFar = text;
    this.post({ type: "status", text });
  }

  private post(msg: unknown): void {
    for (const v of this.views) {
      void v.webview.postMessage(msg);
    }
  }

  private renderHtml(cspSource: string): string {
    const n = nonce();
    return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src ${cspSource} 'unsafe-inline'; script-src 'nonce-${n}';">
<style>
  html, body { height: 100%; }
  body {
    margin: 0; padding: 0; display: flex; flex-direction: column; height: 100vh;
    background: var(--vscode-panel-background); color: var(--vscode-foreground);
    font-family: var(--vscode-font-family, sans-serif);
  }
  #status {
    padding: 4px 8px; font-size: 12px; opacity: 0.75;
    border-bottom: 1px solid var(--vscode-panel-border, transparent);
    white-space: pre-wrap;
  }
  #output {
    flex: 1; overflow-y: auto; padding: 8px;
    white-space: pre-wrap; word-break: break-word;
    font-family: 'Nirmala UI', var(--vscode-editor-font-family, monospace);
    font-size: 14px; line-height: 1.6;
  }
  #inputRow {
    border-top: 1px solid var(--vscode-panel-border, transparent);
    display: flex; align-items: stretch;
  }
  #inputBox {
    flex: 1; min-width: 0; box-sizing: border-box; border: none; outline: none;
    padding: 6px 8px; background: var(--vscode-input-background);
    color: var(--vscode-input-foreground);
    font-family: 'Nirmala UI', var(--vscode-editor-font-family, monospace);
    font-size: 14px;
  }
  #sendBtn {
    border: none; padding: 0 14px; cursor: pointer;
    background: var(--vscode-button-background); color: var(--vscode-button-foreground);
    font-family: 'Nirmala UI', var(--vscode-font-family, sans-serif);
    font-size: 14px; white-space: nowrap;
  }
  #sendBtn:hover { background: var(--vscode-button-hoverBackground); }
</style>
</head>
<body>
  <div id="status"></div>
  <div id="output"></div>
  <div id="inputRow">
    <input id="inputBox" placeholder="খালি রেখে Enter = বর্তমান ফাইল চালাও, বা যেকোনো kolom কমান্ড লিখুন (যেমন: ইনস্টল)">
    <button id="sendBtn">▶ চালাও</button>
  </div>
  <script nonce="${n}">
    const vscodeApi = acquireVsCodeApi();
    const outputEl = document.getElementById('output');
    const statusEl = document.getElementById('status');
    const inputEl = document.getElementById('inputBox');
    const sendBtn = document.getElementById('sendBtn');

    // Chat-style: the same control is "run" while idle and "send" once a
    // program is live and reading পড়ো_লাইন. This mirrors sending a message
    // in a chat box, rather than requiring a separate run action first.
    let running = false;

    function setRunning(isRunning) {
      running = isRunning;
      inputEl.placeholder = running
        ? 'ইনপুট লিখে Enter চাপুন (পড়ো_লাইন-এর জন্য)'
        : 'খালি রেখে Enter = বর্তমান ফাইল চালাও, বা যেকোনো kolom কমান্ড লিখুন (যেমন: ইনস্টল)';
      sendBtn.textContent = running ? '➤ পাঠাও' : '▶ চালাও';
    }

    function submit() {
      const text = inputEl.value;
      if (running) {
        outputEl.textContent += text + '\\n';
        outputEl.scrollTop = outputEl.scrollHeight;
        vscodeApi.postMessage({ type: 'input', text });
      } else {
        // Empty text runs the active .ক file; anything else is a full
        // kolom command line (e.g. ইনস্টল, or যোগ foo https://...).
        vscodeApi.postMessage({ type: 'run', text });
      }
      inputEl.value = '';
    }

    window.addEventListener('message', (event) => {
      const msg = event.data;
      if (msg.type === 'output') {
        outputEl.textContent += msg.text;
        outputEl.scrollTop = outputEl.scrollHeight;
      } else if (msg.type === 'status') {
        statusEl.textContent = msg.text;
      } else if (msg.type === 'clear') {
        outputEl.textContent = '';
        statusEl.textContent = '';
      } else if (msg.type === 'running') {
        setRunning(msg.running);
      }
    });

    inputEl.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        submit();
      }
    });
    sendBtn.addEventListener('click', submit);

    // Tells the extension host our message listener is now attached, so it
    // can safely replay state without racing this page's own load.
    vscodeApi.postMessage({ type: 'ready' });
  </script>
</body>
</html>`;
  }
}
