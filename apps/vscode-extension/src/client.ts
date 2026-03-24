/**
 * HTTP client for communicating with the OCO core server.
 */

interface SessionResponse {
	id: string;
	status: string;
	steps: number;
}

interface StatusResponse {
	status: string;
	steps: number;
	tokensUsed: number;
	toolCallsUsed: number;
	sessionId: string | null;
}

export class OcoClient {
	private baseUrl: string;
	private currentSessionId: string | null = null;

	constructor(baseUrl: string) {
		this.baseUrl = baseUrl.replace(/\/$/, "");
	}

	async startSession(request: string, workspaceRoot?: string): Promise<SessionResponse> {
		const response = await fetch(`${this.baseUrl}/api/v1/sessions`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({
				user_request: request,
				workspace_root: workspaceRoot ?? null,
			}),
		});

		if (!response.ok) {
			throw new Error(`HTTP ${response.status}: ${await response.text()}`);
		}

		const session = (await response.json()) as SessionResponse;
		this.currentSessionId = session.id;
		return session;
	}

	async stopSession(): Promise<void> {
		if (!this.currentSessionId) {
			throw new Error("No active session");
		}

		const response = await fetch(`${this.baseUrl}/api/v1/sessions/${this.currentSessionId}/stop`, {
			method: "POST",
		});

		if (!response.ok) {
			throw new Error(`HTTP ${response.status}: ${await response.text()}`);
		}

		this.currentSessionId = null;
	}

	async getStatus(): Promise<StatusResponse> {
		const response = await fetch(`${this.baseUrl}/api/v1/status`);
		if (!response.ok) {
			throw new Error(`HTTP ${response.status}: ${await response.text()}`);
		}
		return (await response.json()) as StatusResponse;
	}

	async indexWorkspace(workspaceRoot: string): Promise<void> {
		const response = await fetch(`${this.baseUrl}/api/v1/index`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify({ workspace_root: workspaceRoot }),
		});

		if (!response.ok) {
			throw new Error(`HTTP ${response.status}: ${await response.text()}`);
		}
	}

	async getTrace(sessionId?: string): Promise<unknown> {
		const id = sessionId ?? this.currentSessionId;
		if (!id) throw new Error("No session ID");

		const response = await fetch(`${this.baseUrl}/api/v1/sessions/${id}/trace`);
		if (!response.ok) {
			throw new Error(`HTTP ${response.status}: ${await response.text()}`);
		}
		return response.json();
	}
}
