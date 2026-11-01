import { beforeEach, describe, expect, it } from "vitest";
import type { Snapshot } from "../types/generated/Snapshot";
import { useSnapshotStore } from "./snapshotStore";

const snap = (seq: number, phase: Snapshot["phase"] = "idle"): Snapshot => ({
  seq,
  mic: "off",
  phase,
  activeRequestId: null,
  network: "unknown",
  cloudInUse: false,
  pendingApprovals: [],
  lastError: null,
});

describe("snapshotStore", () => {
  beforeEach(() => useSnapshotStore.setState({ snapshot: null }));

  it("accepts the first snapshot and newer ones", () => {
    const { receive } = useSnapshotStore.getState();
    receive(snap(0));
    receive(snap(2, "thinking"));
    expect(useSnapshotStore.getState().snapshot?.phase).toBe("thinking");
  });

  it("ignores stale and duplicate seqs", () => {
    const { receive } = useSnapshotStore.getState();
    receive(snap(5, "speaking"));
    receive(snap(3, "idle"));
    receive(snap(5, "idle"));
    expect(useSnapshotStore.getState().snapshot).toMatchObject({
      seq: 5,
      phase: "speaking",
    });
  });
});
