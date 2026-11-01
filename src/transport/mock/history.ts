import type { ConversationSummary } from "../../types/generated/ConversationSummary";
import type { HistoryMessage } from "../../types/generated/HistoryMessage";
import type { HistoryRetention } from "../../types/generated/HistoryRetention";
import type { HistoryRole } from "../../types/generated/HistoryRole";
import type { JarvisError } from "../../types/generated/JarvisError";

/** A conversation ends after this long without a message (contract). */
const IDLE_MS = 10 * 60 * 1000;
const DAY_MS = 86_400_000;
const DAYS: Record<HistoryRetention, number | null> = {
  off: 0,
  days7: 7,
  days30: 30,
  days365: 365,
  forever: null,
};

function lastOf<T>(list: T[]): T | undefined {
  return list[list.length - 1];
}

interface Conversation {
  summary: ConversationSummary;
  messages: HistoryMessage[];
}

/**
 * Dev-only stand-in for the backend history (spec §8.2): same splitting,
 * retention and delete-all rules, kept in memory only.
 */
export class MockHistory {
  private conversations: Conversation[] = [];
  private count = 0;
  retention: HistoryRetention = "days30";

  constructor(private now: () => number = Date.now) {}

  record(role: HistoryRole, content: string): void {
    if (this.retention === "off" || !content.trim()) return;
    const now = this.now();
    let open = lastOf(this.conversations);
    const last =
      (open && lastOf(open.messages)?.ts) ?? open?.summary.startedAt ?? 0;
    if (!open || open.summary.endedAt !== null || now - last > IDLE_MS) {
      if (open && open.summary.endedAt === null) open.summary.endedAt = last;
      open = {
        summary: {
          id: `mock-conv-${++this.count}`,
          startedAt: now,
          endedAt: null,
          title: null,
          messageCount: 0,
        },
        messages: [],
      };
      this.conversations.push(open);
    }
    if (role === "user" && open.summary.title === null) {
      open.summary.title = [...content.split("\n")[0]].slice(0, 60).join("");
    }
    open.messages.push({
      id: `mock-msg-${++this.count}`,
      role,
      content,
      ts: now,
    });
    open.summary.messageCount = open.messages.length;
  }

  list(limit: number, before?: string): ConversationSummary[] {
    const n = Math.min(200, Math.max(1, Math.floor(limit)));
    let end = this.conversations.length;
    if (before !== undefined) {
      end = this.conversations.findIndex((c) => c.summary.id === before);
      if (end < 0) return [];
    }
    return this.conversations
      .slice(Math.max(0, end - n), end)
      .reverse()
      .map((c) => ({ ...c.summary }));
  }

  get(id: string): HistoryMessage[] {
    const c = this.conversations.find((x) => x.summary.id === id);
    if (!c) {
      const e: JarvisError = {
        code: "NOT_FOUND",
        message: "That conversation no longer exists.",
        retryable: false,
        requestId: null,
      };
      throw e;
    }
    return c.messages.map((m) => ({ ...m }));
  }

  deleteAll(): void {
    this.conversations = [];
  }

  setRetention(r: HistoryRetention): void {
    this.retention = r;
    const days = DAYS[r];
    if (days === null) return;
    const cutoff = this.now() - days * DAY_MS;
    this.conversations = this.conversations.filter(
      (c) =>
        days > 0 &&
        (c.summary.endedAt ?? lastOf(c.messages)?.ts ?? c.summary.startedAt) >=
          cutoff,
    );
  }
}
