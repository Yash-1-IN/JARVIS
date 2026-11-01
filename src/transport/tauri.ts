import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { PROTOCOL_VERSION } from "../protocol";
import type { Envelope } from "../types/generated/Envelope";
import type { EventKind } from "../types/generated/EventKind";
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
import type {
  SnapshotListener,
  Transport,
  TurnListener,
  UsageListener,
  InterruptListener,
  ActivityListener,
  MemoryListener,
} from "./index";

export const SNAPSHOT_EVENT = "state_snapshot";
export const USAGE_EVENT = "usage_updated";
export const INTERRUPT_EVENT = "turn_interrupted";
export const ACTIVITY_EVENT = "activity_appended";
export const MEMORY_PROPOSED_EVENT = "memory_proposed";

export class TauriTransport implements Transport {
  readonly isMock = false;

  ping(): Promise<PingResponse> {
    return invoke<PingResponse>("ping");
  }

  getSnapshot(): Promise<Snapshot> {
    return invoke<Snapshot>("get_snapshot");
  }

  /**
   * The backend broadcasts `Envelope<Snapshot>` with kind `state.snapshot` on
   * the Tauri event `state_snapshot` (event names cannot contain '.').
   * Envelopes from another protocol version are dropped; the startup `ping`
   * check already blocks the UI on a mismatch.
   */
  onSnapshot(listener: SnapshotListener): () => void {
    return subscribe<Snapshot>(SNAPSHOT_EVENT, "state.snapshot", listener);
  }

  onInterrupted(listener: InterruptListener): () => void {
    return subscribe<TurnInterrupted>(
      INTERRUPT_EVENT,
      "turn.interrupted",
      listener,
    );
  }

  onUsage(listener: UsageListener): () => void {
    return subscribe<UsageSummary>(USAGE_EVENT, "usage.updated", listener);
  }

  // Settings commands are agreed in #11; until their Rust handlers land these
  // reject and the settings panel shows the error.
  getSettings(): Promise<Settings> {
    return invoke<Settings>("settings_get");
  }

  updateSettings(patch: SettingsPatch): Promise<Settings> {
    return invoke<Settings>("settings_set", { patch });
  }

  setApiKey(key: string): Promise<ApiKeyStatus> {
    return invoke<ApiKeyStatus>("api_key_set", { key });
  }

  clearApiKey(): Promise<ApiKeyStatus> {
    return invoke<ApiKeyStatus>("api_key_clear");
  }

  listActivity(limit: number, before?: string): Promise<ActivityEntry[]> {
    return invoke<ActivityEntry[]>("activity_list", { limit, before });
  }

  onActivity(listener: ActivityListener): () => void {
    return subscribe<ActivityEntry>(
      ACTIVITY_EVENT,
      "activity.appended",
      listener,
    );
  }

  listMemories(status?: MemoryStatus): Promise<Memory[]> {
    return invoke<Memory[]>("memory_list", { status });
  }

  acceptMemory(id: string): Promise<Memory> {
    return invoke<Memory>("memory_accept", { id });
  }

  editMemory(id: string, content: string): Promise<Memory> {
    return invoke<Memory>("memory_edit", { id, content });
  }

  deleteMemory(id: string): Promise<void> {
    return invoke("memory_delete", { id });
  }

  onMemoryProposed(listener: MemoryListener): () => void {
    return subscribe<Memory>(
      MEMORY_PROPOSED_EVENT,
      "memory.proposed",
      listener,
    );
  }

  exportData(): Promise<string> {
    return invoke<string>("memory_export");
  }

  listConversations(
    limit: number,
    before?: string,
  ): Promise<ConversationSummary[]> {
    return invoke<ConversationSummary[]>("history_list", { limit, before });
  }

  getConversation(conversationId: string): Promise<HistoryMessage[]> {
    return invoke<HistoryMessage[]>("history_get", { conversationId });
  }

  deleteAllHistory(): Promise<void> {
    return invoke("history_delete_all");
  }

  getHistorySettings(): Promise<HistorySettings> {
    return invoke<HistorySettings>("history_settings_get");
  }

  setHistorySettings(settings: HistorySettings): Promise<HistorySettings> {
    return invoke<HistorySettings>("history_settings_set", { settings });
  }

  pickFolder(): Promise<string | null> {
    return invoke<string | null>("folder_pick");
  }

  appsInstalled(): Promise<AllowedApp[]> {
    return invoke<AllowedApp[]>("apps_installed");
  }

  getUsage(): Promise<UsageSummary> {
    return invoke<UsageSummary>("usage_get");
  }

  cancel(requestId: string): Promise<void> {
    return invoke("cancel", { requestId });
  }

  submitText(text: string, onEvent: TurnListener): Promise<string> {
    const channel = new Channel<TurnStreamEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("submit_text", { text, onEvent: channel });
  }

  // Arg name `decision` is this side's reading of docs/contracts.md
  // (`resolve_approval(ApprovalDecision)`); confirm in the M4 contract PR.
  resolveApproval(decision: ApprovalDecision): Promise<void> {
    return invoke("resolve_approval", { decision });
  }

  setMute(muted: boolean): Promise<void> {
    return invoke("set_mute", { muted });
  }
}

/**
 * Listens for one broadcast kind. Envelopes from another protocol version or
 * kind are dropped. Unsubscribe is idempotent and safe before listen() resolves.
 */
function subscribe<P>(
  event: string,
  kind: EventKind,
  listener: (p: P) => void,
): () => void {
  let stopped = false;
  const unlisten = listen<Envelope<P>>(event, (e) => {
    const env = e.payload;
    if (env.v !== PROTOCOL_VERSION || env.kind !== kind) return;
    listener(env.payload);
  });
  return () => {
    if (stopped) return;
    stopped = true;
    void unlisten.then((fn) => fn());
  };
}
