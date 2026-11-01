import type { JarvisError } from "../../types/generated/JarvisError";

/** The M2 fault tests (spec §13), injectable into the next mock turn. */
export const MOCK_FAULTS = {
  "no-key": {
    code: "PROVIDER_AUTH",
    message: "No API key is set.",
    retryable: false,
  },
  "wrong-key": {
    code: "PROVIDER_AUTH",
    message: "The API key was rejected.",
    retryable: false,
  },
  offline: {
    code: "NETWORK_OFFLINE",
    message: "No network connection.",
    retryable: true,
  },
  "http-429": {
    code: "PROVIDER_RATE_LIMIT",
    message: "Rate limited (429) after 2 retries.",
    retryable: true,
  },
  "http-500": {
    code: "PROVIDER_UNAVAILABLE",
    message: "Provider error (500) after 2 retries.",
    retryable: true,
  },
  timeout: {
    code: "PROVIDER_UNAVAILABLE",
    message: "No first token within 10 s.",
    retryable: true,
  },
  "spend-cap": {
    code: "SPEND_CAP_REACHED",
    message: "Today's spend cap is reached.",
    retryable: false,
  },
} satisfies Record<string, Omit<JarvisError, "requestId">>;

export type MockFault = keyof typeof MOCK_FAULTS;
