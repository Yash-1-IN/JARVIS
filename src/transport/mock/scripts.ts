import { z } from "zod";
import approvalHigh from "./scripts/approval-high.json";
import approval from "./scripts/approval.json";
import error from "./scripts/error.json";
import interrupt from "./scripts/interrupt.json";
import offline from "./scripts/offline.json";
import wakeListenThinkSpeak from "./scripts/wake-listen-think-speak.json";

const micState = z.enum([
  "off",
  "muted",
  "standby",
  "capturing",
  "unavailable",
]);
const phase = z.enum([
  "idle",
  "listening",
  "transcribing",
  "thinking",
  "awaiting_approval",
  "acting",
  "speaking",
  "error",
]);
const errorCode = z.enum([
  "MIC_PERMISSION_DENIED",
  "DEVICE_LOST",
  "MODEL_MISSING",
  "MODEL_CHECKSUM",
  "PROVIDER_AUTH",
  "PROVIDER_RATE_LIMIT",
  "PROVIDER_UNAVAILABLE",
  "NETWORK_OFFLINE",
  "SPEND_CAP_REACHED",
  "TOOL_DENIED",
  "TOOL_TIMEOUT",
  "TOOL_FAILED",
  "INTERNAL",
]);

const approvalRequest = z.strictObject({
  approvalId: z.string(),
  requestId: z.string(),
  tool: z.string(),
  risk: z.enum(["medium", "high"]),
  summary: z.string(),
  targets: z.array(z.string()),
  argsDigest: z.string(),
  /** 0 in a script means "60 s after the step is applied". */
  expiresAt: z.number(),
  allowVoice: z.boolean(),
  tainted: z.boolean(),
});

const snapshotPatch = z
  .strictObject({
    mic: micState,
    phase,
    activeRequestId: z.string().nullable(),
    network: z.enum(["online", "offline", "unknown"]),
    cloudInUse: z.boolean(),
    pendingApprovals: z.array(approvalRequest),
    lastError: z
      .strictObject({
        code: errorCode,
        message: z.string(),
        retryable: z.boolean(),
        requestId: z.string().nullable(),
      })
      .nullable(),
  })
  .partial();

const mockScript = z.strictObject({
  name: z.string(),
  description: z.string(),
  steps: z.array(
    z.strictObject({ delayMs: z.number().int().min(0), patch: snapshotPatch }),
  ),
});

export type SnapshotPatch = z.infer<typeof snapshotPatch>;
export type MockScript = z.infer<typeof mockScript>;

/** Parsed at load so a typo in a script fails loudly instead of rendering garbage. */
export const MOCK_SCRIPTS: readonly MockScript[] = [
  wakeListenThinkSpeak,
  interrupt,
  error,
  offline,
  approval,
  approvalHigh,
].map((s) => mockScript.parse(s));
