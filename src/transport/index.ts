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
import type { SpotifyStatus } from "../types/generated/SpotifyStatus";
import type { Transcript } from "../types/generated/Transcript";
import type { TurnInterrupted } from "../types/generated/TurnInterrupted";
import type { TurnStreamEvent } from "../types/generated/TurnStreamEvent";
import type { VoiceStatus } from "../types/generated/VoiceStatus";
import type { ModelProgress } from "../types/generated/ModelProgress";
import type { UsageSummary } from "../types/generated/UsageSummary";

export type SnapshotListener = (snapshot: Snapshot) => void;
export type TurnListener = (event: TurnStreamEvent) => void;
export type UsageListener = (usage: UsageSummary) => void;
export type InterruptListener = (e: TurnInterrupted) => void;
export type ActivityListener = (e: ActivityEntry) => void;
export type MemoryListener = (m: Memory) => void;
export type ModelProgressListener = (p: ModelProgress) => void;
export type TranscriptListener = (t: Transcript) => void;

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
   * Contract `folder_pick`: opens the OS folder picker and returns the chosen
   * absolute path, or null if the user cancelled. Picking allows nothing by
   * itself; add the path with `updateSettings`, which validates it.
   */
  pickFolder(): Promise<string | null>;
  /**
   * Contract `activity_list`: newest first, at most `limit` (1 to 200); with `before`
   * (an entry id) only older entries. An unknown id gives nothing.
   */
  listActivity(limit: number, before?: string): Promise<ActivityEntry[]>;
  /** `activity.appended`: one entry per tool call, including refused ones. */
  onActivity(listener: ActivityListener): () => void;
  /** `usage.updated` broadcasts. Returns an unsubscribe function. */
  onUsage(listener: UsageListener): () => void;

  // Voice (M3, docs/contracts.md "Voice"). Audio never reaches the window: only text.
  /**
   * Contract `voice_attach`: called once at start-up. Turns that start from the microphone stream
   * their reply on `onEvent` (the same events as `submitText`, ending with one `done`).
   */
  voiceAttach(onEvent: TurnListener): Promise<void>;
  /** Contract `voice_capture_start`: push to talk, key down. Refused until `voiceAttach`. */
  voiceCaptureStart(): Promise<void>;
  /** Contract `voice_capture_stop`: key up. Safe when nothing is capturing. */
  voiceCaptureStop(): Promise<void>;
  /** `transcript.final`: the words heard; they open the voice turn the attached channel then streams. */
  onTranscriptFinal(listener: TranscriptListener): () => void;

  /** Contract `voice_status`: text and settings only; `modelsReady` is read from disk each time. */
  voiceStatus(): Promise<VoiceStatus>;
  /**
   * Contract `voice_models_install`: downloads and verifies the speech models. Resolves when
   * every file is verified; progress arrives on `onModelsProgress`. An interrupted download
   * resumes on the next call. One download at a time.
   */
  voiceModelsInstall(): Promise<VoiceStatus>;
  /** `voice.models_progress`: bytes of the archive done so far, at most every 250 ms. */
  onModelsProgress(listener: ModelProgressListener): () => void;

  // Spotify (music step 2, docs/contracts.md "Spotify"). No call ever returns a token.
  /** Contract `spotify_status`: the saved Client ID and whether a sign-in is held. */
  spotifyStatus(): Promise<SpotifyStatus>;
  /** Contract `spotify_set_client_id`: `null` or an empty string clears it. */
  spotifySetClientId(clientId: string | null): Promise<SpotifyStatus>;
  /**
   * Contract `spotify_connect`: opens the sign-in in the system browser and resolves when
   * it ends, times out (about 3 minutes) or is cancelled by `spotifyDisconnect`.
   */
  spotifyConnect(): Promise<SpotifyStatus>;
  /** Contract `spotify_disconnect`: forgets the sign-in and cancels one in progress. */
  spotifyDisconnect(): Promise<SpotifyStatus>;

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
