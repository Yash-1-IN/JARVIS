import { PROTOCOL_VERSION } from "../../protocol";
import type { AllowedApp } from "../../types/generated/AllowedApp";
import type { ApiKeyStatus } from "../../types/generated/ApiKeyStatus";
import type { ApprovalDecision } from "../../types/generated/ApprovalDecision";
import type { ErrorCode } from "../../types/generated/ErrorCode";
import type { JarvisError } from "../../types/generated/JarvisError";
import type { PingResponse } from "../../types/generated/PingResponse";
import type { Settings } from "../../types/generated/Settings";
import type { SettingsPatch } from "../../types/generated/SettingsPatch";
import type { Snapshot } from "../../types/generated/Snapshot";
import type { StopReason } from "../../types/generated/StopReason";
import type { InterruptReason } from "../../types/generated/InterruptReason";
import type { TurnInterrupted } from "../../types/generated/TurnInterrupted";
import type { UsageSummary } from "../../types/generated/UsageSummary";
import type {
  SnapshotListener,
  Transport,
  TurnListener,
  UsageListener,
} from "../index";
import { MOCK_FAULTS, type MockFault } from "./faults";
import { MOCK_SCRIPTS, type MockScript, type SnapshotPatch } from "./scripts";

const APPROVAL_TTL_MS = 60_000;
const FIRST_TOKEN_MS = 400;
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
  private nextFault: MockFault | null = null;
  private usageListeners = new Set<UsageListener>();
  /** Approval ids already decided: a decision is single-use. */
  private decided = new Set<string>();
  private interruptListeners = new Set<(e: TurnInterrupted) => void>();
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

  /** An unanswered approval lapses: nothing runs and the turn ends. */
  private expire(approvalId: string): void {
    if (
      !this.snapshot.pendingApprovals.some((a) => a.approvalId === approvalId)
    )
      return;
    this.decided.add(approvalId);
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
