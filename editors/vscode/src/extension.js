const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

let client;

function activate(context) {
  const command = vscode.workspace.getConfiguration("yan").get("server.path", "yan");
  const run = { command, args: ["lsp"], transport: TransportKind.stdio };
  client = new LanguageClient(
    "yan",
    "Yan",
    { run, debug: run },
    {
      documentSelector: ["yan", "yan-agent"].map((language) => ({ language })),
    },
  );
  context.subscriptions.push(client);
  client.start().catch((err) =>
    vscode.window.showErrorMessage(`Could not start \`${command} lsp\`: ${err.message}`),
  );
}

function deactivate() {
  return client?.stop();
}

module.exports = { activate, deactivate };
