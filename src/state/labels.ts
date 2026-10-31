import type { AssistantPhase } from "../types/generated/AssistantPhase";
import type { MicState } from "../types/generated/MicState";
import type { NetworkState } from "../types/generated/NetworkState";

// Every state has a text label so nothing is conveyed by colour alone.
export const MIC_LABELS: Record<MicState, string> = {
  off: "Mic off",
  muted: "Muted",
  standby: "Waiting for wake word",
  capturing: "Mic live",
  unavailable: "Mic unavailable",
};

export const PHASE_LABELS: Record<AssistantPhase, string> = {
  idle: "Idle",
  listening: "Listening",
  transcribing: "Transcribing",
  thinking: "Thinking",
  awaiting_approval: "Waiting for your approval",
  acting: "Working",
  speaking: "Speaking",
  error: "Something went wrong",
};

export const NETWORK_LABELS: Record<NetworkState, string> = {
  online: "Online",
  offline: "Offline: local features only",
  unknown: "Network unknown",
};
