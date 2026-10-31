import { create } from "zustand";
import type { Snapshot } from "../types/generated/Snapshot";

interface SnapshotStore {
  snapshot: Snapshot | null;
  /** Replaces the copy only with a newer snapshot; stale or duplicate seqs are ignored. */
  receive: (next: Snapshot) => void;
}

// The frontend never transitions assistant state itself; it only mirrors the
// newest snapshot from the backend (CLAUDE.md, architecture rules).
export const useSnapshotStore = create<SnapshotStore>((set, get) => ({
  snapshot: null,
  receive: (next) => {
    const current = get().snapshot;
    if (current && next.seq <= current.seq) return;
    set({ snapshot: next });
  },
}));
