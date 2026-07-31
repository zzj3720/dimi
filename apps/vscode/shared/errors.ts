import type { ErrorPhase } from "./types";

// Pre-flight: task didn't start at all or was blocked by "gatekeeper"
export const PREFLIGHT_CODES = new Set<string>([
  "config.invalid",
  "model.not_configured",
  "auth.login_required",
  "session.not_found",
  "session.state_not_found",
  "session.state_invalid",
  "session.init_failed",
  "shell.git_bash_not_found",
]);

// User-friendly error messages
export const ERROR_MESSAGES: Record<string, string> = {
  "config.invalid": "Kimi Code configuration is invalid.",
  "model.not_configured": "No model is configured. Please sign in or configure a provider.",
  "auth.login_required": "Authentication failed. Please sign in.",
  "session.not_found": "Session was not found.",
  "session.state_not_found": "Session data is missing.",
  "session.state_invalid": "Session data is invalid.",
  "session.init_failed": "Failed to initialize the session.",
  "session.closed": "Session was closed.",
  "session.fork_active_turn": "Wait for the current response before forking.",
  "turn.agent_busy": "A message is being sent. Please wait.",
  "provider.api_error": "Service temporarily unavailable.",
  "provider.rate_limit": "Too many requests. Please try again later.",
  "provider.auth_error": "Authentication failed. Please sign in again.",
  "provider.connection_error": "Could not connect to the model provider.",
  "request.prompt_input_empty": "Prompt cannot be empty.",
  internal: "Internal error occurred.",
};

export function classifyError(code: string): ErrorPhase {
  return PREFLIGHT_CODES.has(code) ? "preflight" : "runtime";
}

export function getUserMessage(code: string, fallback?: string): string {
  return ERROR_MESSAGES[code] || fallback || "An unknown error occurred.";
}

export function isPreflightError(code: string): boolean {
  return PREFLIGHT_CODES.has(code);
}

export function isUserInterrupt(code: string): boolean {
  return code === "turn.cancelled";
}
