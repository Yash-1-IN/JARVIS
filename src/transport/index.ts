import type { ActivityEntry } from "../types/generated/ActivityEntry";
import type { AllowedApp } from "../types/generated/AllowedApp";
import type { ApiKeyStatus } from "../types/generated/ApiKeyStatus";
import type { ApprovalDecision } from "../types/generated/ApprovalDecision";
import type { ConversationSummary } from "../types/generated/ConversationSummary";
import type { HistoryMessage } from "../types/generated/HistoryMessage";
import type { HistorySettings } from "../types/generated/HistorySettings";
import type { Memory } from "../types/generated/Memory";
import type { MemoryStatus } from "../types/generated/MemoryStatus";
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
export type ActivityListener = (e: ActivityEntry) => void;
export type MemoryListener = (m: Memory) => void;

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
  /**
   * Contract `activity_list`: newest first, at most `limit` (1 to 200); with `before`
   * (an entry id) only older entries. An unknown id gives nothing.
   */
  listActivity(limit: number, before?: string): Promise<ActivityEntry[]>;
  /** `activity.appended`: one entry per tool call, including refused ones. */
  onActivity(listener: ActivityListener): () => void;
  /** `usage.updated` broadcasts. Returns an unsubscribe function. */
  onUsage(listener: UsageListener): () => void;

  // Memory (M5, docs/contracts.md "Memory and history").
  /** Contract `memory_list`: newest change first, optionally one status only. */
  listMemories(status?: MemoryStatus): Promise<Memory[]>;
  /** Contract `memory_accept`: a proposed memory becomes active. */
  acceptMemory(id: string): Promise<Memory>;
  /** Contract `memory_edit`: 1 to 500 code points; the status is kept. */
  editMemory(id: string, content: string): Promise<Memory>;
  /** Contract `memory_delete`: hard delete. Denying a proposal is this call. */
  deleteMemory(id: string): Promise<void>;
  /** `memory.proposed`: show the confirmation card with the exact text. */
  onMemoryProposed(listener: MemoryListener): () => void;
  /** Contract `memory_export`: one JSON document of memories and history. */
  exportData(): Promise<string>;

  // History (M5).
  /** Contract `history_list`: newest first; `before` is a conversation id. */
  listConversations(
    limit: number,
    before?: string,
  ): Promise<ConversationSummary[]>;
  /** Contract `history_get`: oldest first; an unknown id is `NOT_FOUND`. */
  getConversation(conversationId: string): Promise<HistoryMessage[]>;
  /** Contract `history_delete_all`: memories and settings stay. */
  deleteAllHistory(): Promise<void>;
  getHistorySettings(): Promise<HistorySettings>;
  /** A shorter period deletes what is now too old at once. */
  setHistorySettings(settings: HistorySettings): Promise<HistorySettings>;
}
