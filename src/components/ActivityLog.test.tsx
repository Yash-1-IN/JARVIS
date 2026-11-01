import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MockTransport } from "../transport/mock/MockTransport";
import { ActivityLog } from "./ActivityLog";

async function approveOnce(t: MockTransport, decision: "approve" | "deny") {
  t.runScript("approval");
  vi.advanceTimersByTime(1000);
  const a = (await t.getSnapshot()).pendingApprovals[0];
  await t.resolveApproval({
    approvalId: a.approvalId,
    argsDigest: a.argsDigest,
    decision,
  });
  vi.advanceTimersByTime(5000);
}

describe("ActivityLog", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("starts empty and shows new entries live, newest first, as words", async () => {
    const t = new MockTransport();
    render(<ActivityLog transport={t} />);
    await act(async () => {});
    expect(screen.getByTestId("activity-empty")).toBeInTheDocument();

    await act(async () => {
      await approveOnce(t, "approve");
      t.simulateRefusedCall();
    });
    const rows = screen.getAllByTestId("activity-entry");
    expect(rows.map((r) => r.dataset.decision)).toEqual([
      "prohibited",
      "approved",
    ]);
    expect(rows[0]).toHaveTextContent("Refused");
    expect(rows[0]).toHaveTextContent("shell.run");
    expect(rows[1]).toHaveTextContent("You approved, done");
    expect(rows[1]).toHaveTextContent("~/Downloads/inv-01.pdf");
  });

  it("records denied and expired approvals", async () => {
    const t = new MockTransport();
    await approveOnce(t, "deny");
    t.runScript("approval");
    vi.advanceTimersByTime(1000 + 60_000);
    render(<ActivityLog transport={t} />);
    await act(async () => {});
    const decisions = screen
      .getAllByTestId("activity-entry")
      .map((r) => r.dataset.decision);
    expect(decisions).toEqual(["expired", "denied"]);
    expect(screen.getAllByTestId("activity-entry")[0]).toHaveTextContent(
      "Approval expired",
    );
  });

  it("pages older entries with Show older", async () => {
    const t = new MockTransport();
    for (let i = 0; i < 60; i++) t.simulateRefusedCall();
    render(<ActivityLog transport={t} />);
    await act(async () => {});
    expect(screen.getAllByTestId("activity-entry")).toHaveLength(50);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Show older" }));
    });
    expect(screen.getAllByTestId("activity-entry")).toHaveLength(60);
    expect(screen.queryByRole("button", { name: "Show older" })).toBeNull();
  });

  it("renders a hostile summary or target as text", async () => {
    const t = new MockTransport();
    vi.spyOn(t, "listActivity").mockResolvedValue([
      {
        id: "x",
        ts: 0,
        requestId: "r",
        tool: "files.move",
        risk: "medium",
        summary: '<img src=x onerror="alert(1)">',
        targets: ["<script>alert(2)</script>"],
        decision: "denied",
        outcome: "not_run",
        durationMs: 0,
      },
    ]);
    const { container } = render(<ActivityLog transport={t} />);
    await act(async () => {});
    expect(container.querySelector("img, script")).toBeNull();
    expect(
      screen.getByText('<img src=x onerror="alert(1)">'),
    ).toBeInTheDocument();
  });
});

describe("MockTransport.listActivity follows the contract", () => {
  it("is newest first, pages with before, clamps limit, unknown id gives nothing", async () => {
    const t = new MockTransport();
    for (let i = 0; i < 5; i++) t.simulateRefusedCall();
    const all = await t.listActivity(10);
    expect(all.map((e) => e.id)).toEqual([
      "mock-act-000005",
      "mock-act-000004",
      "mock-act-000003",
      "mock-act-000002",
      "mock-act-000001",
    ]);
    expect(
      (await t.listActivity(2, "mock-act-000004")).map((e) => e.id),
    ).toEqual(["mock-act-000003", "mock-act-000002"]);
    expect(await t.listActivity(0)).toHaveLength(1);
    expect(await t.listActivity(5, "nope")).toEqual([]);
  });
});
