import { describe, expect, it } from "vitest";
import { MockHistory } from "./history";

describe("MockHistory (same rules as the backend)", () => {
  it("splits after ten idle minutes, titles from the first user message", () => {
    let now = 1_000_000;
    const h = new MockHistory(() => now);
    h.record("user", "first question\nsecond line");
    h.record("assistant", "answer");
    now += 10 * 60 * 1000; // exactly ten minutes: same conversation
    h.record("user", "again");
    now += 10 * 60 * 1000 + 1;
    h.record("user", "new topic");
    const list = h.list(10);
    expect(list.map((c) => c.title)).toEqual(["new topic", "first question"]);
    expect(list[1]).toMatchObject({ messageCount: 3 });
    expect(list[1].endedAt).not.toBeNull();
    expect(list[0].endedAt).toBeNull();
  });

  it("stores nothing when off and purges at once when shortened", () => {
    let now = 0;
    const h = new MockHistory(() => now);
    h.record("user", "old");
    now = 10 * 86_400_000;
    h.record("user", "recent");
    h.setRetention("days7");
    expect(h.list(10).map((c) => c.title)).toEqual(["recent"]);
    h.setRetention("off");
    expect(h.list(10)).toEqual([]);
    h.record("user", "not kept");
    expect(h.list(10)).toEqual([]);
  });

  it("an unknown conversation is NOT_FOUND", () => {
    expect(() => new MockHistory().get("nope")).toThrow(
      expect.objectContaining({ code: "NOT_FOUND" }),
    );
  });
});
