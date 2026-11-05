import { PROTOCOL_VERSION } from "../../protocol";
import type { ActivityEntry } from "../../types/generated/ActivityEntry";
import type { AllowedApp } from "../../types/generated/AllowedApp";
import type { ApprovalRequest } from "../../types/generated/ApprovalRequest";
import type { ToolDecision } from "../../types/generated/ToolDecision";
import type { ToolOutcome } from "../../types/generated/ToolOutcome";
import type { ApiKeyStatus } from "../../types/generated/ApiKeyStatus";
import type { ApprovalDecision } from "../../types/generated/ApprovalDecision";
import type { ErrorCode } from "../../types/generated/ErrorCode";
import type { JarvisError } from "../../types/generated/JarvisError";
import type { ConversationSummary } from "../../types/generated/ConversationSummary";
import type { HistoryMessage } from "../../types/generated/HistoryMessage";
import type { HistorySettings } from "../../types/generated/HistorySettings";
import type { Memory } from "../../types/generated/Memory";
import type { MemoryStatus } from "../../types/generated/MemoryStatus";
import type { PingResponse } from "../../types/generated/PingResponse";
import type { Settings } from "../../types/generated/Settings";
import type { SettingsPatch } from "../../types/generated/SettingsPatch";
import type { Snapshot } from "../../types/generated/Snapshot";
import type { SpotifyStatus } from "../../types/generated/SpotifyStatus";
import type { StopReason } from "../../types/generated/StopReason";
import type { InterruptReason } from "../../types/generated/InterruptReason";
import type { ModelProgress } from "../../types/generated/ModelProgress";
import type { VoiceStatus } from "../../types/generated/VoiceStatus";
import type { Transcript } from "../../types/generated/Transcript";
import type { TurnInterrupted } from "../../types/generated/TurnInterrupted";
import type { UsageSummary } from "../../types/generated/UsageSummary";
import type {
  MemoryListener,
  SnapshotListener,
  Transport,
  TurnListener,
  UsageListener,
} from "../index";
import { MOCK_FAULTS, type MockFault } from "./faults";
import { MockHistory } from "./history";
import { MockMemoryStore, rememberText } from "./memory";
import { MOCK_SCRIPTS, type MockScript, type SnapshotPatch } from "./scripts";

const APPROVAL_TTL_MS = 60_000;
const FIRST_TOKEN_MS = 400;
/** How long the mock microphone takes to open, as a real device can. */
const MOCK_MIC_OPEN_MS = 400;
/** The mock "hears a quiet gap" this long after the microphone opens and ends the capture itself. */
const MOCK_END_OF_SPEECH_MS = 3000;
const TOKEN_MS = 60;
/** What one mock reply "costs", so the usage panel and caps can be exercised. */
const MOCK_TURN_COST_MICRO_USD = 1_500;
const MOCK_TURN_TOKENS = { input: 300, output: 40 };
const MIN_KEY_LEN_FOR_HINT = 16;

const DAY_MS = 86_400_000;

/** What the mock pretends is installed (the real list comes from platform/). */
const MOCK_INSTALLED_APPS: AllowedApp[] = [
  { id: "mock.app.music", name: "Mock Music" },
  { id: "mock.app.notes", name: "Mock Notes" },
];

function nextUtcMidnight(now: number): number {
  return Math.floor(now / DAY_MS) * DAY_MS + DAY_MS;
}

function nextUtcMonth(now: number): number {
  const d = new Date(now);
  return Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 1);
}

/** Rejects like Tauri does: with the serialised JarvisError as the value. */
function jarvisError(
  code: ErrorCode,
  message: string,
  retryable = false,
): JarvisError {
  return { code, message, retryable, requestId: null };
}

function validCap(v: number | null): boolean {
  return v === null || (Number.isSafeInteger(v) && v >= 0);
}

function initialSnapshot(): Snapshot {
  return {
    seq: 0,
    mic: "off",
    phase: "idle",
    activeRequestId: null,
    network: "unknown",
    cloudInUse: false,
    pendingApprovals: [],
    lastError: null,
  };
}

/**
 * Dev-only stand-in for the Rust backend (spec §5.5). It replays scripted
 * snapshots; it never decides state transitions on its own.
 */
export class MockTransport implements Transport {
  readonly isMock = true;
  readonly scripts: readonly MockScript[];
  readonly faults = Object.keys(MOCK_FAULTS) as MockFault[];
  private snapshot = initialSnapshot();
  private listeners = new Set<SnapshotListener>();
  private timers: ReturnType<typeof setTimeout>[] = [];
  private turn: { requestId: string; onEvent: TurnListener } | null = null;
  private turnCount = 0;
  private voiceListener: TurnListener | null = null;
  private modelsReady = false;
  private installing = false;
  private progressListeners = new Set<(p: ModelProgress) => void>();
  private transcriptListeners = new Set<(t: Transcript) => void>();
  private micBeforeCapture: Snapshot["mic"] = "off";
  private nextFault: MockFault | null = null;
  private usageListeners = new Set<UsageListener>();
  /** Approval ids already decided: a decision is single-use. */
  private decided = new Set<string>();
  /** Oldest first, like the backend's log; `listActivity` returns newest first. */
  private activity: ActivityEntry[] = [];
  private activityListeners = new Set<(e: ActivityEntry) => void>();
  private activityCount = 0;
  private interruptListeners = new Set<(e: TurnInterrupted) => void>();
  private memory = new MockMemoryStore();
  private history = new MockHistory();
  private settings: Settings = {
    schemaVersion: 1,
    apiKey: { isSet: false, storage: null, hint: null },
    model: "mock-model-fast",
    availableModels: ["mock-model-fast", "mock-model-smart"],
    dailyCapMicroUsd: null,
    monthlyCapMicroUsd: null,
    allowedFolders: [],
    allowedApps: [],
    autoAllowMedium: false,
  };
  private spent = {
    today: 0,
    month: 0,
    todayIn: 0,
    todayOut: 0,
    monthIn: 0,
    monthOut: 0,
  };

  constructor(scripts: readonly MockScript[] = MOCK_SCRIPTS) {
    this.scripts = scripts;
  }

  async ping(): Promise<PingResponse> {
    return {
      pong: true,
      protocolVersion: PROTOCOL_VERSION,
      appVersion: "mock",
    };
  }

  async getSnapshot(): Promise<Snapshot> {
    return this.snapshot;
  }

  onSnapshot(listener: SnapshotListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** Cancels any running script and plays `name` from its first step. */
  runScript(name: string): void {
    const script = this.scripts.find((s) => s.name === name);
    if (!script) throw new Error(`Unknown mock script: ${name}`);
    this.stop();
    let at = 0;
    for (const step of script.steps) {
      at += step.delayMs;
      this.timers.push(setTimeout(() => this.apply(step.patch), at));
    }
  }

  /** The next submitText fails with this fault instead of replying. */
  failNextTurn(fault: MockFault | null): void {
    this.nextFault = fault;
  }

  /** Streams a canned, clearly labelled reply word by word. */
  async submitText(text: string, onEvent: TurnListener): Promise<string> {
    this.stop();
    this.endTurn("cancelled");
    const requestId = `mock-turn-${++this.turnCount}`;
    this.turn = { requestId, onEvent };

    // "Remember that ..." is answered locally, like the backend: no cloud, no
    // cost, works at the spend cap. The card shows the text, the reply doesn't;
    // history keeps a neutral line so the text cannot outlive the memory.
    const remember = rememberText(text);
    this.history.record(
      "user",
      remember === null
        ? text
        : "Asked JARVIS to remember something (see Memory).",
    );
    if (remember !== null) {
      let reply = "Okay. Confirm it on the card and I will keep it.";
      try {
        this.memory.propose(remember);
      } catch (e) {
        reply = (e as JarvisError).message;
      }
      this.apply({
        phase: "thinking",
        activeRequestId: requestId,
        lastError: null,
      });
      this.timers.push(
        setTimeout(() => {
          onEvent({ type: "text_delta", text: reply });
          this.history.record("assistant", reply);
          this.endTurn("end");
          this.apply({ phase: "idle", activeRequestId: null });
        }, 100),
      );
      return requestId;
    }

    this.apply({
      phase: "thinking",
      activeRequestId: requestId,
      cloudInUse: true,
      lastError: null,
    });

    const fault = this.capReached() ? "spend-cap" : this.nextFault;
    if (fault) {
      if (fault === this.nextFault) this.nextFault = null;
      const err = MOCK_FAULTS[fault];
      this.timers.push(
        setTimeout(() => {
          this.endTurn(
            err.code === "SPEND_CAP_REACHED" ? "spend_cap" : "error",
          );
          this.apply({
            phase: "error",
            activeRequestId: null,
            cloudInUse: false,
            network:
              err.code === "NETWORK_OFFLINE"
                ? "offline"
                : this.snapshot.network,
            lastError: { ...err, requestId },
          });
        }, FIRST_TOKEN_MS),
      );
      return requestId;
    }

    const reply = `(MOCK reply, not a model) You said: "${text.trim()}".`;
    const words = reply.split(/(?<= )/);
    words.forEach((word, i) => {
      this.timers.push(
        setTimeout(
          () => onEvent({ type: "text_delta", text: word }),
          FIRST_TOKEN_MS + i * TOKEN_MS,
        ),
      );
    });
    this.timers.push(
      setTimeout(
        () => {
          this.history.record("assistant", reply);
          this.endTurn("end");
          this.recordUsage();
          this.apply({
            phase: "idle",
            activeRequestId: null,
            cloudInUse: false,
          });
        },
        FIRST_TOKEN_MS + words.length * TOKEN_MS,
      ),
    );
    return requestId;
  }

  private voiceStatusNow(): VoiceStatus {
    return {
      wakeEnabled: false,
      followUpWindow: true,
      voice: "",
      availableVoices: [],
      inputDevice: null,
      micPermission: "unknown",
      modelsReady: this.modelsReady,
    };
  }

  async voiceStatus(): Promise<VoiceStatus> {
    return this.voiceStatusNow();
  }

  /** Nothing is downloaded: a short, labelled run that ends with the models "ready". */
  async voiceModelsInstall(): Promise<VoiceStatus> {
    if (this.installing) {
      throw jarvisError(
        "INTERNAL",
        "The speech models are already being downloaded.",
      );
    }
    this.installing = true;
    const total = 250_807_309;
    const steps = 5;
    try {
      for (let i = 1; i <= steps; i++) {
        await new Promise<void>((r) => this.timers.push(setTimeout(r, 200)));
        const p = {
          name: "Speech recognition (MOCK: nothing is downloaded)",
          doneBytes: Math.floor((total * i) / steps),
          totalBytes: total,
        };
        for (const l of this.progressListeners) l(p);
      }
      this.modelsReady = true;
      return this.voiceStatusNow();
    } finally {
      this.installing = false;
    }
  }

  onModelsProgress(listener: (p: ModelProgress) => void): () => void {
    this.progressListeners.add(listener);
    return () => this.progressListeners.delete(listener);
  }

  async voiceAttach(onEvent: TurnListener): Promise<void> {
    this.voiceListener = onEvent;
  }

  async voiceCaptureStart(): Promise<void> {
    if (!this.voiceListener) {
      throw {
        code: "INTERNAL",
        message: "Voice input is not ready yet.",
        retryable: true,
        requestId: null,
      } satisfies JarvisError;
    }
    if (this.snapshot.phase === "listening") return;
    if (this.snapshot.activeRequestId || this.snapshot.mic === "muted") {
      throw {
        code: "INTERNAL",
        message: "JARVIS is busy with another request.",
        retryable: true,
        requestId: null,
      } satisfies JarvisError;
    }
    this.micBeforeCapture = this.snapshot.mic;
    // As the backend does: listening at once, `capturing` only once the device has opened.
    this.apply({ phase: "listening", lastError: null });
    this.timers.push(
      setTimeout(() => {
        if (this.snapshot.phase === "listening") {
          this.apply({ mic: "capturing" });
        }
      }, MOCK_MIC_OPEN_MS),
    );
    // Like the backend's end-of-speech detector: a capture nobody stops ends by itself.
    this.timers.push(
      setTimeout(
        () => void this.voiceCaptureStop(),
        MOCK_MIC_OPEN_MS + MOCK_END_OF_SPEECH_MS,
      ),
    );
  }

  /** Nothing is heard in the mock: key up plays a fixed, labelled sentence as the transcript. */
  async voiceCaptureStop(): Promise<void> {
    const listener = this.voiceListener;
    if (!listener || this.snapshot.phase !== "listening") return;
    this.apply({ phase: "transcribing", mic: this.micBeforeCapture });
    const text = "(MOCK: nothing was heard) what time is it";
    const requestId = await this.submitText(text, listener);
    for (const l of this.transcriptListeners) l({ requestId, text });
  }

  onTranscriptFinal(listener: (t: Transcript) => void): () => void {
    this.transcriptListeners.add(listener);
    return () => this.transcriptListeners.delete(listener);
  }

  async cancel(requestId: string): Promise<void> {
    if (this.snapshot.activeRequestId !== requestId) return;
    this.interrupt("user_cancel");
  }

  onInterrupted(listener: (e: TurnInterrupted) => void): () => void {
    this.interruptListeners.add(listener);
    return () => this.interruptListeners.delete(listener);
  }

  /**
   * Ends the active turn early as the backend would (contracts.md): the turn
   * channel gets `done cancelled`, `turn.interrupted` is broadcast, and the
   * phase becomes `listening` for a wake word and `idle` otherwise.
   */
  interrupt(reason: InterruptReason): void {
    const requestId = this.snapshot.activeRequestId;
    if (!requestId) return;
    this.stop();
    this.endTurn("cancelled");
    const wake = reason === "wake_word";
    this.apply({
      phase: wake ? "listening" : "idle",
      activeRequestId: null,
      mic: wake
        ? "capturing"
        : this.snapshot.mic === "capturing"
          ? "standby"
          : this.snapshot.mic,
      cloudInUse: false,
      pendingApprovals: [],
    });
    for (const l of this.interruptListeners) l({ requestId, reason });
  }

  /**
   * Mirrors the gateway rules (spec §9.3): the decision must echo the shown
   * argsDigest, arrive before expiry, and be the first for that approval.
   */
  async resolveApproval(d: ApprovalDecision): Promise<void> {
    const req = this.snapshot.pendingApprovals.find(
      (a) => a.approvalId === d.approvalId,
    );
    if (!req || this.decided.has(d.approvalId)) {
      throw jarvisError(
        "TOOL_DENIED",
        "This approval is no longer pending; nothing was run.",
      );
    }
    if (d.argsDigest !== req.argsDigest) {
      throw jarvisError(
        "TOOL_DENIED",
        "The action changed since it was shown; nothing was run.",
      );
    }
    if (Date.now() >= req.expiresAt) {
      this.expire(req.approvalId);
      throw jarvisError(
        "TOOL_DENIED",
        "The approval expired; nothing was run.",
      );
    }
    this.decided.add(d.approvalId);
    this.logActivity(
      req,
      d.decision === "approve" ? "approved" : "denied",
      d.decision === "approve" ? "ok" : "not_run",
    );
    const rest = this.snapshot.pendingApprovals.filter(
      (a) => a.approvalId !== d.approvalId,
    );
    if (d.decision === "approve") {
      this.apply({ pendingApprovals: rest, phase: "acting" });
      this.timers.push(
        setTimeout(() => this.apply({ phase: "thinking" }), 1500),
      );
      this.timers.push(
        setTimeout(
          () => this.apply({ phase: "idle", activeRequestId: null }),
          2500,
        ),
      );
    } else {
      this.apply({ pendingApprovals: rest, phase: "thinking" });
      this.timers.push(
        setTimeout(
          () => this.apply({ phase: "idle", activeRequestId: null }),
          800,
        ),
      );
    }
  }

  async listActivity(limit: number, before?: string): Promise<ActivityEntry[]> {
    const n = Math.min(200, Math.max(1, Math.floor(limit)));
    let end = this.activity.length;
    if (before !== undefined) {
      end = this.activity.findIndex((e) => e.id === before);
      if (end < 0) return [];
    }
    return this.activity.slice(Math.max(0, end - n), end).reverse();
  }

  onActivity(listener: (e: ActivityEntry) => void): () => void {
    this.activityListeners.add(listener);
    return () => this.activityListeners.delete(listener);
  }

  async listMemories(status?: MemoryStatus): Promise<Memory[]> {
    return this.memory.list(status);
  }

  async acceptMemory(id: string): Promise<Memory> {
    return this.memory.accept(id);
  }

  async editMemory(id: string, content: string): Promise<Memory> {
    return this.memory.edit(id, content);
  }

  async deleteMemory(id: string): Promise<void> {
    this.memory.delete(id);
  }

  onMemoryProposed(listener: MemoryListener): () => void {
    return this.memory.onProposed(listener);
  }

  async exportData(): Promise<string> {
    const history = this.history.list(200).map((c) => ({
      conversation: c,
      messages: this.history.get(c.id),
    }));
    return JSON.stringify(
      {
        format: "jarvis-export",
        version: 1,
        exportedAt: Date.now(),
        historyRetention: this.history.retention,
        memories: this.memory.list(),
        history,
      },
      null,
      2,
    );
  }

  async listConversations(
    limit: number,
    before?: string,
  ): Promise<ConversationSummary[]> {
    return this.history.list(limit, before);
  }

  async getConversation(conversationId: string): Promise<HistoryMessage[]> {
    return this.history.get(conversationId);
  }

  async deleteAllHistory(): Promise<void> {
    this.history.deleteAll();
  }

  async getHistorySettings(): Promise<HistorySettings> {
    return { retention: this.history.retention };
  }

  async setHistorySettings(
    settings: HistorySettings,
  ): Promise<HistorySettings> {
    this.history.setRetention(settings.retention);
    return { retention: this.history.retention };
  }

  // Spotify. MOCK: nothing is contacted; Connect "signs in" as a made-up account.
  private spotify: SpotifyStatus = {
    clientId: null,
    connected: false,
    displayName: null,
  };

  async spotifyStatus(): Promise<SpotifyStatus> {
    return { ...this.spotify };
  }

  async spotifySetClientId(clientId: string | null): Promise<SpotifyStatus> {
    const id = (clientId ?? "").trim().toLowerCase();
    if (id !== "" && !/^[0-9a-f]{32}$/.test(id)) {
      throw jarvisError(
        "INTERNAL",
        "That does not look like a Spotify Client ID (32 characters, 0-9 and a-f)",
      );
    }
    const next = id === "" ? null : id;
    if (next !== this.spotify.clientId) {
      this.spotify = { clientId: next, connected: false, displayName: null };
    }
    return { ...this.spotify };
  }

  async spotifyConnect(): Promise<SpotifyStatus> {
    if (this.spotify.clientId === null) {
      throw jarvisError("INTERNAL", "Add your Spotify Client ID first.");
    }
    this.spotify = {
      ...this.spotify,
      connected: true,
      displayName: "Mock listener",
    };
    return { ...this.spotify };
  }

  async spotifyDisconnect(): Promise<SpotifyStatus> {
    this.spotify = { ...this.spotify, connected: false, displayName: null };
    return { ...this.spotify };
  }

  /** The next `pickFolder` answer; `null` acts like Cancel. */
  nextPickedFolder: string | null = "/Users/mock/Documents/Mock Folder";

  async pickFolder(): Promise<string | null> {
    return this.nextPickedFolder;
  }

  /** Dev helper: the model proposes a memory (`memory.propose`), as in a cloud turn. */
  simulateMemoryProposal(): void {
    this.memory.propose("Prefers short answers before 9 am.");
  }

  /** Dev helper: the model reached for a tool that does not exist (logged, never run). */
  simulateRefusedCall(): void {
    this.pushActivity({
      requestId: this.snapshot.activeRequestId ?? "mock-req-none",
      tool: "shell.run",
      risk: "high",
      summary: "Refused: not a known tool",
      targets: [],
      decision: "prohibited",
      outcome: "not_run",
      durationMs: 0,
    });
  }

  private logActivity(
    req: ApprovalRequest,
    decision: ToolDecision,
    outcome: ToolOutcome,
  ): void {
    this.pushActivity({
      requestId: req.requestId,
      tool: req.tool,
      risk: req.risk,
      summary: req.summary,
      targets: req.targets,
      decision,
      outcome,
      durationMs: outcome === "ok" ? 1500 : 0,
    });
  }

  private pushActivity(e: Omit<ActivityEntry, "id" | "ts">): void {
    const entry: ActivityEntry = {
      ...e,
      id: `mock-act-${String(++this.activityCount).padStart(6, "0")}`,
      ts: Date.now(),
    };
    this.activity.push(entry);
    for (const l of this.activityListeners) l(entry);
  }

  /** An unanswered approval lapses: nothing runs and the turn ends. */
  private expire(approvalId: string): void {
    if (
      !this.snapshot.pendingApprovals.some((a) => a.approvalId === approvalId)
    )
      return;
    this.decided.add(approvalId);
    const req = this.snapshot.pendingApprovals.find(
      (a) => a.approvalId === approvalId,
    );
    if (req) this.logActivity(req, "expired", "not_run");
    const rest = this.snapshot.pendingApprovals.filter(
      (a) => a.approvalId !== approvalId,
    );
    this.apply({
      pendingApprovals: rest,
      ...(rest.length === 0 && {
        phase: "idle" as const,
        activeRequestId: null,
      }),
    });
  }

  async setMute(muted: boolean): Promise<void> {
    if (this.snapshot.mic === "unavailable") return;
    this.apply({ mic: muted ? "muted" : "standby" });
  }

  async getSettings(): Promise<Settings> {
    return structuredClone(this.settings);
  }

  async updateSettings(patch: SettingsPatch): Promise<Settings> {
    if (
      patch.model !== undefined &&
      !this.settings.availableModels.includes(patch.model)
    ) {
      throw jarvisError("INTERNAL", `Unknown model: ${patch.model}`);
    }
    for (const cap of [patch.dailyCapMicroUsd, patch.monthlyCapMicroUsd]) {
      if (cap !== undefined && !validCap(cap)) {
        throw jarvisError(
          "INTERNAL",
          "A spend cap must be a whole, non-negative amount.",
        );
      }
    }
    // Contract #19: folders must be absolute; apps must be installed ids, and the
    // name always comes from the installed list, never from the patch.
    if (
      patch.allowedFolders?.some((f) => !/^(\/|[A-Za-z]:[\\/])/.test(f.trim()))
    ) {
      throw jarvisError("INTERNAL", "Folders must be absolute paths.");
    }
    const apps = patch.allowedApps?.map((a) => {
      const known = MOCK_INSTALLED_APPS.find((i) => i.id === a.id);
      if (!known) throw jarvisError("INTERNAL", `Unknown app: ${a.id}`);
      return { ...known };
    });
    const next = { ...this.settings };
    if (patch.allowedFolders !== undefined) {
      next.allowedFolders = patch.allowedFolders.map((f) => f.trim());
    }
    if (apps !== undefined) next.allowedApps = apps;
    if (patch.autoAllowMedium !== undefined)
      next.autoAllowMedium = patch.autoAllowMedium;
    if (patch.model !== undefined) next.model = patch.model;
    if (patch.dailyCapMicroUsd !== undefined)
      next.dailyCapMicroUsd = patch.dailyCapMicroUsd;
    if (patch.monthlyCapMicroUsd !== undefined)
      next.monthlyCapMicroUsd = patch.monthlyCapMicroUsd;
    this.settings = next;
    this.emitUsage();
    return structuredClone(next);
  }

  /**
   * Mirrors the contract: trim, check format, "validate" (keys starting with
   * `bad` are rejected; offline refuses), and keep only the hint, never the key.
   */
  async setApiKey(key: string): Promise<ApiKeyStatus> {
    const k = key.trim();
    if (!k || k.length > 512 || /\s/.test(k)) {
      throw jarvisError("PROVIDER_AUTH", "That doesn't look like an API key.");
    }
    if (this.snapshot.network === "offline") {
      throw jarvisError(
        "NETWORK_OFFLINE",
        "Can't check the key while offline.",
        true,
      );
    }
    if (k.startsWith("bad")) {
      throw jarvisError("PROVIDER_AUTH", "The provider rejected this key.");
    }
    this.settings.apiKey = {
      isSet: true,
      storage: "keychain",
      hint: k.length >= MIN_KEY_LEN_FOR_HINT ? `…${k.slice(-4)}` : null,
    };
    return { ...this.settings.apiKey };
  }

  async clearApiKey(): Promise<ApiKeyStatus> {
    this.settings.apiKey = { isSet: false, storage: null, hint: null };
    return { ...this.settings.apiKey };
  }

  async appsInstalled(): Promise<AllowedApp[]> {
    return MOCK_INSTALLED_APPS.map((a) => ({ ...a }));
  }

  async getUsage(): Promise<UsageSummary> {
    return this.usageSummary();
  }

  onUsage(listener: UsageListener): () => void {
    this.usageListeners.add(listener);
    return () => this.usageListeners.delete(listener);
  }

  /** Dev helper: pretend the key is stored only for this session (D-09 warning path). */
  simulateSessionOnlyKey(): void {
    if (this.settings.apiKey.isSet)
      this.settings.apiKey.storage = "session_only";
  }

  private capReached(): boolean {
    const { dailyCapMicroUsd: d, monthlyCapMicroUsd: m } = this.settings;
    return (
      (d !== null && this.spent.today >= d) ||
      (m !== null && this.spent.month >= m)
    );
  }

  private recordUsage(): void {
    this.spent.today += MOCK_TURN_COST_MICRO_USD;
    this.spent.month += MOCK_TURN_COST_MICRO_USD;
    this.spent.todayIn += MOCK_TURN_TOKENS.input;
    this.spent.todayOut += MOCK_TURN_TOKENS.output;
    this.spent.monthIn += MOCK_TURN_TOKENS.input;
    this.spent.monthOut += MOCK_TURN_TOKENS.output;
    this.emitUsage();
  }

  private usageSummary(): UsageSummary {
    const now = Date.now();
    return {
      todayMicroUsd: this.spent.today,
      monthMicroUsd: this.spent.month,
      todayInputTokens: this.spent.todayIn,
      todayOutputTokens: this.spent.todayOut,
      monthInputTokens: this.spent.monthIn,
      monthOutputTokens: this.spent.monthOut,
      dailyCapMicroUsd: this.settings.dailyCapMicroUsd,
      monthlyCapMicroUsd: this.settings.monthlyCapMicroUsd,
      dayResetsAt: nextUtcMidnight(now),
      monthResetsAt: nextUtcMonth(now),
    };
  }

  private emitUsage(): void {
    const u = this.usageSummary();
    for (const l of this.usageListeners) l(u);
  }

  private endTurn(stopReason: StopReason): void {
    this.turn?.onEvent({ type: "done", stopReason });
    this.turn = null;
  }

  stop(): void {
    this.timers.forEach(clearTimeout);
    this.timers = [];
  }

  private apply(patch: SnapshotPatch): void {
    const now = Date.now();
    const pendingApprovals = patch.pendingApprovals?.map((a) => ({
      ...a,
      expiresAt: a.expiresAt === 0 ? now + APPROVAL_TTL_MS : a.expiresAt,
    }));
    this.snapshot = {
      ...this.snapshot,
      ...patch,
      ...(pendingApprovals && { pendingApprovals }),
      seq: this.snapshot.seq + 1,
    };
    for (const a of pendingApprovals ?? []) {
      this.timers.push(
        setTimeout(() => this.expire(a.approvalId), a.expiresAt - now),
      );
    }
    for (const l of this.listeners) l(this.snapshot);
  }
}
