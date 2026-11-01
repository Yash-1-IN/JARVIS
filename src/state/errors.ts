import type { ErrorCode } from "../types/generated/ErrorCode";

export interface ErrorInfo {
  title: string;
  /** What the user can do about it (spec §7.3). */
  hint: string;
}

// Record over the closed enum: adding an ErrorCode in Rust fails typecheck here.
export const ERROR_INFO: Record<ErrorCode, ErrorInfo> = {
  MIC_PERMISSION_DENIED: {
    title: "Microphone blocked",
    hint: "Allow microphone access for JARVIS in your system settings.",
  },
  DEVICE_LOST: {
    title: "Audio device disconnected",
    hint: "Reconnect it or pick another device.",
  },
  MODEL_MISSING: {
    title: "Speech model missing",
    hint: "Download it from Settings → Models.",
  },
  MODEL_CHECKSUM: {
    title: "Speech model damaged",
    hint: "Re-download it from Settings → Models.",
  },
  PROVIDER_AUTH: {
    title: "API key problem",
    hint: "Add or replace your API key in Settings.",
  },
  PROVIDER_RATE_LIMIT: {
    title: "AI provider is busy",
    hint: "Wait a moment, then retry.",
  },
  PROVIDER_UNAVAILABLE: {
    title: "AI provider not responding",
    hint: "Retry, or try again later.",
  },
  NETWORK_OFFLINE: {
    title: "You're offline",
    hint: "Local commands still work; cloud answers resume when you reconnect.",
  },
  SPEND_CAP_REACHED: {
    title: "Spend cap reached",
    hint: "Raise the cap in Settings, or wait for it to reset. Local commands still work.",
  },
  TOOL_DENIED: { title: "Action not allowed", hint: "Nothing was changed." },
  TOOL_TIMEOUT: {
    title: "Action timed out",
    hint: "Check whether it finished before retrying.",
  },
  TOOL_FAILED: {
    title: "Action failed",
    hint: "See the activity log for what ran.",
  },
  NOT_FOUND: {
    title: "Not found",
    hint: "That item no longer exists. Refresh the list.",
  },
  INTERNAL: {
    title: "Something went wrong",
    hint: "Retry; if it keeps happening, check the logs.",
  },
};

function isJarvisError(e: unknown): e is { code: ErrorCode; message: string } {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as { code?: unknown }).code === "string" &&
    (e as { code: string }).code in ERROR_INFO &&
    typeof (e as { message?: unknown }).message === "string"
  );
}

/**
 * Tauri rejects a command with the serialised `JarvisError` object, not an
 * `Error`, so `String(e)` would print "[object Object]".
 */
export function describeError(e: unknown): string {
  if (isJarvisError(e)) return `${ERROR_INFO[e.code].title}: ${e.message}`;
  if (e instanceof Error) return e.message;
  return String(e);
}
