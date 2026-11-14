// The TypeScript 7 extension sends a file to TypeScript's server only when an extension has registered the file's
// extension with it. This registers `.contract`, so a project's content mapper (editors/content-mapper) is asked
// about a contract opened on its own. It does so only once the workspace turns content mappers on; until then, or
// without that extension or its mapper API, it does nothing and wakes no other extension.
//
// It also formats a contract with the Contract compiler (`contract fmt --stdin`), once the workspace names that
// compiler in `exactContract.compiler`; until then no formatter is registered and nothing is run.
const { execFile } = require('node:child_process');

const TYPESCRIPT = 'TypeScriptTeam.native-preview', ID = 'exact.exact-contract', SETTING = 'js/ts.contentMappers.enabled';
const LANGUAGE = 'exact-contract', COMPILER = 'exactContract.compiler';

/** Registers `.contract` with the TypeScript 7 extension, if the setting is on and that extension is installed.
 * `vscode` is the API (`extensions`, `workspace`); a registration is made at most once. */
async function register(vscode, context, state = {}) {
  if (state.registered || !vscode.workspace.getConfiguration().get(SETTING)) return false;
  const api = await vscode.extensions.getExtension(TYPESCRIPT)?.activate();
  const registration = api?.registerContentMappers?.(ID, [{ extensions: ['.contract'] }]);
  if (!registration || state.registered) { registration?.dispose(); return false; }
  state.registered = true;
  context.subscriptions.push(registration);
  return true;
}

/** `text` as `contract fmt --stdin` writes it, the compiler being `bin`; none when the compiler refuses the text or
 * cannot be run, and `missing` is told when it is not there. `file` names the text and is not read. */
const formatted = (bin, file, text, missing) => new Promise(done => {
  const child = execFile(bin, ['fmt', '--stdin', file], { maxBuffer: 64 << 20 }, (error, stdout) => {
    if (error?.code === 'ENOENT') missing();
    done(error ? null : stdout);
  });
  child.stdin.on('error', () => {}); // a compiler that did not start has no stdin to write; `execFile` reports it
  child.stdin.end(text);
});

/** Keeps a formatter for contracts registered while the workspace names a compiler, and none otherwise. */
function format(vscode, state) {
  state.formatter?.dispose();
  state.formatter = undefined;
  const bin = vscode.workspace.getConfiguration().get(COMPILER);
  if (!bin) return;
  let said = false;
  const missing = () => {
    if (!said) vscode.window.showWarningMessage(`Exact Contract: \`${COMPILER}\` names no Contract compiler: ${bin}`);
    said = true;
  };
  state.formatter = vscode.languages.registerDocumentFormattingEditProvider(LANGUAGE, {
    // A refused text gets no edit: its error is the compiler's, shown where its others are.
    async provideDocumentFormattingEdits(document) {
      const text = document.getText(), after = await formatted(bin, document.fileName, text, missing);
      if (after === null || after === text) return [];
      return [vscode.TextEdit.replace(new vscode.Range(document.positionAt(0), document.positionAt(text.length)), after)];
    },
  });
}

/** Registers now, and again when either setting changes. */
function activate(context, vscode = require('vscode')) {
  const state = {};
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(e => {
      if (e.affectsConfiguration(SETTING)) register(vscode, context, state);
      if (e.affectsConfiguration(COMPILER)) format(vscode, state);
    }),
    { dispose: () => state.formatter?.dispose() },
  );
  format(vscode, state);
  return register(vscode, context, state);
}

module.exports = { register, activate };
