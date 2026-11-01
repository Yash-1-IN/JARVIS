import { describe, expect, it } from "vitest";
import { describeError } from "./errors";
import { capToInput, formatReset, formatUsd, parseCapInput } from "./money";

describe("money helpers", () => {
  it("formats micro-dollars, keeping sub-cent amounts visible", () => {
    expect(formatUsd(0)).toBe("$0.00");
    expect(formatUsd(2_500_000)).toBe("$2.50");
    expect(formatUsd(1_500)).toBe("$0.0015");
  });

  it("parses caps typed in dollars", () => {
    expect(parseCapInput("")).toBeNull();
    expect(parseCapInput("  ")).toBeNull();
    expect(parseCapInput("2")).toBe(2_000_000);
    expect(parseCapInput("$2.5")).toBe(2_500_000);
    expect(parseCapInput("0.07")).toBe(70_000);
    expect(parseCapInput("0.0018")).toBe(1_800);
    expect(parseCapInput("0.000001")).toBe(1);
    for (const bad of ["-1", "abc", "0.0000001", "1.", ".5", "1e3", "1,5"]) {
      expect(parseCapInput(bad), bad).toBe("invalid");
    }
  });

  it("round-trips a cap through the form", () => {
    expect(capToInput(null)).toBe("");
    expect(capToInput(2_000_000)).toBe("2.00");
    expect(capToInput(2_500_000)).toBe("2.50");
    expect(capToInput(1_800)).toBe("0.0018");
    for (const micro of [0, 1, 1_800, 12_340_000, 123_456_789]) {
      expect(parseCapInput(capToInput(micro)), String(micro)).toBe(micro);
    }
  });

  it("shows only the time for a reset later today, else the date too", () => {
    const now = new Date(2026, 10, 1, 9, 0).getTime();
    expect(
      formatReset(new Date(2026, 10, 1, 17, 30).getTime(), now),
    ).not.toMatch(/Nov/);
    expect(formatReset(new Date(2026, 11, 1, 5, 30).getTime(), now)).toMatch(
      /Dec/,
    );
  });
});

describe("describeError", () => {
  it("formats a JarvisError object as Tauri rejects with it", () => {
    expect(
      describeError({
        code: "PROVIDER_AUTH",
        message: "Rejected.",
        retryable: false,
        requestId: null,
      }),
    ).toBe("API key problem: Rejected.");
  });

  it("falls back for Error objects and strings", () => {
    expect(describeError(new Error("boom"))).toBe("boom");
    expect(describeError("command x not found")).toBe("command x not found");
    expect(describeError({ code: "NOT_A_CODE", message: "m" })).toBe(
      "[object Object]",
    );
  });
});
