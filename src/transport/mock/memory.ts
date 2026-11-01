import type { JarvisError } from "../../types/generated/JarvisError";
import type { Memory } from "../../types/generated/Memory";
import type { MemoryStatus } from "../../types/generated/MemoryStatus";
import { MAX_MEMORY_CHARS } from "../../state/memory";

function fail(code: "NOT_FOUND" | "INTERNAL", message: string): JarvisError {
  return { code, message, retryable: false, requestId: null };
}

/** One line, no control characters, within the limit; or a JarvisError. */
function clean(text: string): string {
  // eslint-disable-next-line no-control-regex
  const one = text.replace(/[\u0000-\u001f\u007f-\u009f]/g, " ");
  const s = one.split(/\s+/).filter(Boolean).join(" ");
  if (!s) throw fail("INTERNAL", "The memory text is empty.");
  if ([...s].length > MAX_MEMORY_CHARS) {
    throw fail(
      "INTERNAL",
      `A memory can be at most ${MAX_MEMORY_CHARS} characters.`,
    );
  }
  return s;
}

/**
 * Dev-only stand-in for the backend memory store (spec §8.4), with the same
 * rules: every new memory is `proposed`, the same text is one memory, delete is
 * a hard delete, and an unknown id is `NOT_FOUND`.
 */
export class MockMemoryStore {
  private rows: Memory[] = [];
  private count = 0;
  private listeners = new Set<(m: Memory) => void>();

  onProposed(listener: (m: Memory) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  /** "Remember that ..." or the model's `memory.propose`. */
  propose(text: string): Memory {
    const content = clean(text);
    const same = this.rows.find(
      (m) => m.content === content && m.status !== "archived",
    );
    if (same) {
      if (same.status === "proposed") this.announce(same);
      return same;
    }
    const now = Date.now();
    const m: Memory = {
      id: `mock-mem-${++this.count}`,
      content,
      status: "proposed",
      createdAt: now,
      updatedAt: now,
    };
    this.rows.push(m);
    this.announce(m);
    return m;
  }

  list(status?: MemoryStatus): Memory[] {
    return this.rows
      .filter((m) => !status || m.status === status)
      .sort((a, b) => b.updatedAt - a.updatedAt || (a.id < b.id ? 1 : -1));
  }

  accept(id: string): Memory {
    return this.update(id, { status: "active" });
  }

  edit(id: string, content: string): Memory {
    const text = clean(content);
    this.get(id);
    if (
      this.rows.some(
        (m) => m.id !== id && m.content === text && m.status !== "archived",
      )
    ) {
      throw fail("INTERNAL", "You already have that memory.");
    }
    return this.update(id, { content: text });
  }

  delete(id: string): void {
    this.get(id);
    this.rows = this.rows.filter((m) => m.id !== id);
  }

  private get(id: string): Memory {
    const m = this.rows.find((r) => r.id === id);
    if (!m) throw fail("NOT_FOUND", "That memory no longer exists.");
    return m;
  }

  private update(id: string, patch: Partial<Memory>): Memory {
    const m = { ...this.get(id), ...patch, updatedAt: Date.now() };
    this.rows = this.rows.map((r) => (r.id === id ? m : r));
    return m;
  }

  private announce(m: Memory): void {
    for (const l of this.listeners) l({ ...m });
  }
}

/** The text after "remember that", exactly as typed, like the backend intent. */
export function rememberText(input: string): string | null {
  const t = input.trim();
  for (const p of [
    "please remember that ",
    "jarvis remember that ",
    "remember that ",
    "remember: ",
  ]) {
    if (t.slice(0, p.length).toLowerCase() === p) {
      const rest = t.slice(p.length).trim();
      if (rest) return rest;
    }
  }
  return null;
}
