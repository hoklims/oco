import * as vscode from "vscode";

/**
 * WebView panel for displaying OCO decision traces.
 */
export class TracePanel {
	private panel: vscode.WebviewPanel | undefined;
	private extensionUri: vscode.Uri;

	constructor(extensionUri: vscode.Uri) {
		this.extensionUri = extensionUri;
	}

	show(): void {
		if (this.panel) {
			this.panel.reveal();
			return;
		}

		this.panel = vscode.window.createWebviewPanel(
			"ocoTrace",
			"OCO Decision Trace",
			vscode.ViewColumn.Two,
			{
				enableScripts: true,
				retainContextWhenHidden: true,
			},
		);

		this.panel.webview.html = this.getHtml();

		this.panel.onDidDispose(() => {
			this.panel = undefined;
		});
	}

	updateTrace(traceData: unknown): void {
		this.panel?.webview.postMessage({
			type: "updateTrace",
			data: traceData,
		});
	}

	dispose(): void {
		this.panel?.dispose();
	}

	private getHtml(): string {
		return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>OCO Decision Trace</title>
    <style>
        body {
            font-family: var(--vscode-font-family);
            color: var(--vscode-foreground);
            background: var(--vscode-editor-background);
            padding: 16px;
        }
        .step {
            border: 1px solid var(--vscode-panel-border);
            border-radius: 4px;
            margin-bottom: 8px;
            padding: 12px;
        }
        .step-header {
            font-weight: bold;
            margin-bottom: 4px;
        }
        .action-type {
            color: var(--vscode-textLink-foreground);
        }
        .reason {
            color: var(--vscode-descriptionForeground);
            font-size: 0.9em;
        }
        .budget {
            font-size: 0.8em;
            color: var(--vscode-descriptionForeground);
        }
        h2 { margin-top: 0; }
        #empty-state {
            text-align: center;
            color: var(--vscode-descriptionForeground);
            margin-top: 40px;
        }
    </style>
</head>
<body>
    <h2>Decision Trace</h2>
    <div id="trace-container">
        <div id="empty-state">No trace data yet. Start a session to see decisions.</div>
    </div>
    <script>
        const container = document.getElementById('trace-container');
        const emptyState = document.getElementById('empty-state');

        window.addEventListener('message', event => {
            const { type, data } = event.data;
            if (type === 'updateTrace' && data) {
                emptyState.style.display = 'none';
                renderTrace(data);
            }
        });

        function renderTrace(traces) {
            const items = Array.isArray(traces) ? traces : [traces];
            for (const trace of items) {
                const el = document.createElement('div');
                el.className = 'step';
                el.innerHTML =
                    '<div class="step-header">Step ' + trace.step +
                    ' — <span class="action-type">' + (trace.action?.type || 'unknown') +
                    '</span></div>' +
                    '<div class="reason">' + (trace.reason || '') + '</div>' +
                    '<div class="budget">Tokens: ' + (trace.budget_snapshot?.tokens_used || 0) +
                    ' / Tool calls: ' + (trace.budget_snapshot?.tool_calls_used || 0) + '</div>';
                container.appendChild(el);
            }
        }
    </script>
</body>
</html>`;
	}
}
