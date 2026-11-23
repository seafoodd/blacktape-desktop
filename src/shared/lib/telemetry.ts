export function isCancellationError(error: unknown): boolean {
  if (!error) return false;
  const msg = error instanceof Error ? error.message : String(error);
  const lower = msg.toLowerCase();
  return (
    lower.includes("auth_window_closed") ||
    lower.includes("auth window closed") ||
    lower.includes("user_cancelled") ||
    lower.includes("user cancelled")
  );
}

export function captureFrontendError(error: unknown, context?: string): void {
  if (context) {
    console.error(`[${context}]`, error);
  } else {
    console.error(error);
  }

  // Do not report user cancellations (e.g. user closed auth window) to Sentry
  if (isCancellationError(error)) {
    return;
  }

  // Only forward when tauri-plugin-sentry injected the bridge (i.e. SENTRY_DSN was set at build time)
  const hasSentryBridge =
    typeof window !== "undefined" &&
    Boolean((window as unknown as { __SENTRY__?: unknown }).__SENTRY__);
  if (!hasSentryBridge) return;

  // Rethrow asynchronously so tauri-plugin-sentry's global error listener captures it
  // and passes it to Rust `sanitize_event` without interrupting the caller's control flow.
  const normalized = error instanceof Error ? error : new Error(String(error));
  if (context) {
    normalized.message = `[${context}] ${normalized.message}`;
  }

  queueMicrotask(() => {
    throw normalized;
  });
}
