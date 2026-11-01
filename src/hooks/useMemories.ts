import { useCallback, useEffect, useState } from "react";
import { describeError } from "../state/errors";
import type { Transport } from "../transport";
import type { Memory } from "../types/generated/Memory";

/** True for the backend's "that memory no longer exists" rejection. */
function isNotFound(e: unknown): boolean {
  return (
    typeof e === "object" &&
    e !== null &&
    (e as { code?: unknown }).code === "NOT_FOUND"
  );
}

function newestFirst(list: Memory[]): Memory[] {
  return [...list].sort((a, b) => b.updatedAt - a.updatedAt);
}

export interface Memories {
  /** Every memory the backend has, newest change first; null while loading. */
  memories: Memory[] | null;
  loadError: string | null;
  accept(id: string): Promise<void>;
  edit(id: string, content: string): Promise<void>;
  remove(id: string): Promise<void>;
}

/**
 * The one copy of the memory list in the window (M5). The proposal card and the
 * Memory panel both read it, so accepting on the card shows in the panel at
 * once. Actions reject with the backend's error (show it with
 * `describeError`); a memory that no longer exists is dropped from the list.
 */
export function useMemories(transport: Transport): Memories {
  const [memories, setMemories] = useState<Memory[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const put = useCallback((m: Memory) => {
    setMemories((list) =>
      newestFirst([...(list ?? []).filter((x) => x.id !== m.id), m]),
    );
  }, []);
  const drop = useCallback((id: string) => {
    setMemories((list) => list && list.filter((x) => x.id !== id));
  }, []);

  useEffect(() => {
    let live = true;
    // Subscribe first so a proposal between the two calls is not lost.
    const off = transport.onMemoryProposed((m) => live && put(m));
    // Through a promise so a synchronous failure lands in `catch` as well.
    Promise.resolve()
      .then(() => transport.listMemories())
      .then((list) => {
        if (!live) return;
        // Merge: a proposal that arrived first stays if the list missed it.
        setMemories((seen) =>
          newestFirst([
            ...list,
            ...(seen ?? []).filter((s) => !list.some((m) => m.id === s.id)),
          ]),
        );
      })
      .catch((e: unknown) => live && setLoadError(describeError(e)));
    return () => {
      live = false;
      off();
    };
  }, [transport, put]);

  const act = useCallback(
    async (id: string, run: () => Promise<Memory | void>) => {
      try {
        const m = await run();
        if (m) put(m);
        else drop(id);
      } catch (e) {
        if (isNotFound(e)) drop(id);
        throw e;
      }
    },
    [put, drop],
  );

  return {
    memories,
    loadError,
    accept: (id) => act(id, () => transport.acceptMemory(id)),
    edit: (id, content) => act(id, () => transport.editMemory(id, content)),
    remove: (id) => act(id, () => transport.deleteMemory(id)),
  };
}

/** Length as the backend counts it: Unicode code points, not UTF-16 units. */
export function codePoints(s: string): number {
  return [...s].length;
}
