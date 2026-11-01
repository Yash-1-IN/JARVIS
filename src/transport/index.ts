import type { AllowedApp } from "../types/generated/AllowedApp";
import type { ApiKeyStatus } from "../types/generated/ApiKeyStatus";
import type { ApprovalDecision } from "../types/generated/ApprovalDecision";
import type { PingResponse } from "../types/generated/PingResponse";
import type { Settings } from "../types/generated/Settings";
import type { SettingsPatch } from "../types/generated/SettingsPatch";
import type { Snapshot } from "../types/generated/Snapshot";
import type { TurnInterrupted } from "../types/generated/TurnInterrupted";
import type { TurnStreamEvent } from "../types/generated/TurnStreamEvent";
import type { UsageSummary } from "../types/generated/UsageSummary";

export type SnapshotListener = (snapshot: Snapshot) => void;
export type TurnListener = (event: TurnStreamEvent) => void;
export type UsageListener = (usage: UsageSummary) => void;
export type InterruptListener = (e: TurnInterrupted) => void;

/** One interface, two implementations: TauriTransport and MockTransport. */
export interface Transport {
  /** True only for MockTransport; drives the persistent MOCK badge. */
  readonly isMock: boolean;
  ping(): Promise<PingResponse>;
  getSnapshot(): Promise<Snapshot>;
  /** Subscribe to snapshot updates. Returns an unsubscribe function. */
  onSnapshot(listener: SnapshotListener): () => void;
  /** Cancels the whole request tree for `requestId` (contract: `cancel`). */
  cancel(requestId: string): Promise<void>;
  /**
   * Starts a text turn (contract: `submit_text`). Stream events arrive on
   * `onEvent`, possibly before the returned requestId resolves; the stream
   * always ends with exactly one `done`.
   */
  submitText(text: string, onEvent: TurnListener): Promise<string>;
  /**
   * `turn.interrupted` broadcasts: why a turn stopped early. The turn's own
   * channel still ends with `done { stopReason: "cancelled" }`.
   */
  onInterrupted(listener: InterruptListener): () => void;
  /**
   * Contract: `resolve_approval`. Must echo the request's `argsDigest`; the
   * backend rejects a mismatch, an expired request, or a second decision.
   */
  resolveApproval(decision: ApprovalDecision): Promise<void>;
  /** Contract: `set_mute`. */
  setMute(muted: boolean): Promise<void>;

  // Settings (contract #11, docs/contracts.md "Settings and API key").
  getSettings(): Promise<Settings>;
  /** Omit a field to leave it unchanged; `null` clears a cap. */
  updateSettings(patch: SettingsPatch): Promise<Settings>;
  /**
   * The only time the key crosses IPC. The backend validates it and stores it
   * only on success; callers must not keep the key after this call.
   */
  setApiKey(key: string): Promise<ApiKeyStatus>;
  clearApiKey(): Promise<ApiKeyStatus>;
  getUsage(): Promise<UsageSummary>;
  /**
   * Contract `apps_installed`: what may go on the app allow-list. Only these ids are
   * accepted by `settings_set`, and the name always comes from this list.
   */
  appsInstalled(): Promise<AllowedApp[]>;
  /** `usage.updated` broadcasts. Returns an unsubscribe function. */
  onUsage(listener: UsageListener): () => void;
}
