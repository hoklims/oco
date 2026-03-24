import * as vscode from "vscode";
import { OcoClient } from "./client";
import { TracePanel } from "./trace-panel";

let client: OcoClient | undefined;
let tracePanel: TracePanel | undefined;

export function activate(context: vscode.ExtensionContext): void {
	const config = vscode.workspace.getConfiguration("oco");
	const coreUrl = config.get<string>("coreUrl", "http://127.0.0.1:3000");

	client = new OcoClient(coreUrl);

	context.subscriptions.push(
		vscode.commands.registerCommand("oco.startSession", async () => {
			if (!client) return;
			const request = await vscode.window.showInputBox({
				prompt: "What would you like to do?",
				placeHolder: "e.g., Explain how auth tokens are refreshed",
			});
			if (!request) return;

			const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
			try {
				const session = await client.startSession(request, workspaceRoot);
				vscode.window.showInformationMessage(`OCO session started: ${session.id}`);
			} catch (err) {
				vscode.window.showErrorMessage(`Failed to start session: ${err}`);
			}
		}),

		vscode.commands.registerCommand("oco.stopSession", async () => {
			if (!client) return;
			try {
				await client.stopSession();
				vscode.window.showInformationMessage("OCO session stopped");
			} catch (err) {
				vscode.window.showErrorMessage(`Failed to stop session: ${err}`);
			}
		}),

		vscode.commands.registerCommand("oco.showTrace", () => {
			if (!tracePanel) {
				tracePanel = new TracePanel(context.extensionUri);
			}
			tracePanel.show();
		}),

		vscode.commands.registerCommand("oco.showStatus", async () => {
			if (!client) return;
			try {
				const status = await client.getStatus();
				vscode.window.showInformationMessage(
					`OCO: ${status.status} | Steps: ${status.steps} | Tokens: ${status.tokensUsed}`,
				);
			} catch (err) {
				vscode.window.showErrorMessage(`Failed to get status: ${err}`);
			}
		}),

		vscode.commands.registerCommand("oco.indexWorkspace", async () => {
			if (!client) return;
			const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
			if (!workspaceRoot) {
				vscode.window.showWarningMessage("No workspace folder open");
				return;
			}
			try {
				await client.indexWorkspace(workspaceRoot);
				vscode.window.showInformationMessage("Workspace indexed successfully");
			} catch (err) {
				vscode.window.showErrorMessage(`Indexing failed: ${err}`);
			}
		}),
	);

	// Auto-index on activation if configured
	const autoIndex = config.get<boolean>("autoIndex", true);
	if (autoIndex && vscode.workspace.workspaceFolders?.[0]) {
		client.indexWorkspace(vscode.workspace.workspaceFolders[0].uri.fsPath).catch(() => {
			// Silent fail on auto-index — not critical
		});
	}
}

export function deactivate(): void {
	client = undefined;
	tracePanel?.dispose();
	tracePanel = undefined;
}
