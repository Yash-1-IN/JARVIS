import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import css from "../styles.css?raw";
import type { ApprovalRequest } from "../types/generated/ApprovalRequest";
import { ApprovalCard } from "./ApprovalCard";

const NOW = 1_000_000;
const req = (over: Partial<ApprovalRequest> = {}): ApprovalRequest => ({
  approvalId: "a1",
  requestId: "r1",
  tool: "files.move",
  risk: "medium",
  summary: "Move 3 files from Downloads to Documents.",
  targets: ["~/Downloads/a.pdf", "~/Documents/"],
  argsDigest: "d1",
  expiresAt: NOW + 60_000,
  allowVoice: true,
  tainted: false,
  ...over,
});

describe("ApprovalCard", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW);
  });
  afterEach(() => vi.useRealTimers());

  it("shows the backend summary, exact targets, risk as text and the countdown", () => {
    render(<ApprovalCard request={req()} onDecide={vi.fn()} />);
    expect(
      screen.getByText("Move 3 files from Downloads to Documents."),
    ).toBeInTheDocument();
    expect(screen.getByText("~/Downloads/a.pdf")).toBeInTheDocument();
    expect(screen.getByText("Medium risk")).toBeInTheDocument();
    expect(screen.getByTestId("approval-timer")).toHaveTextContent("60s left");
    act(() => {
      vi.advanceTimersByTime(15_000);
    });
    expect(screen.getByTestId("approval-timer")).toHaveTextContent("45s left");
  });

  it("does not focus Approve, so a stray keypress cannot approve", () => {
    render(<ApprovalCard request={req()} onDecide={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Approve" })).not.toHaveFocus();
    expect(document.activeElement).toBe(document.body);
  });

  it("is single-use: one decision, then both buttons lock", async () => {
    const onDecide = vi.fn().mockResolvedValue(undefined);
    render(<ApprovalCard request={req()} onDecide={onDecide} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    });
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(onDecide).toHaveBeenCalledTimes(1);
    expect(onDecide).toHaveBeenCalledWith("approve");
    expect(screen.getByRole("button", { name: "Deny" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Approved");
  });

  it("locks after expiry and sends nothing", () => {
    const onDecide = vi.fn();
    render(<ApprovalCard request={req()} onDecide={onDecide} />);
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(screen.getByTestId("approval-timer")).toHaveTextContent("Expired");
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(onDecide).not.toHaveBeenCalled();
  });

  it("marks high risk as on-screen only and warns when tainted", () => {
    render(
      <ApprovalCard
        request={req({ risk: "high", allowVoice: false, tainted: true })}
        onDecide={vi.fn()}
      />,
    );
    expect(screen.getByText("High risk")).toBeInTheDocument();
    expect(screen.getByText(/voice can't approve this/)).toBeInTheDocument();
    expect(screen.getByTestId("tainted-warning")).toBeInTheDocument();
  });

  it("renders a hostile summary as text, not HTML", () => {
    const { container } = render(
      <ApprovalCard
        request={req({ summary: '<img src=x onerror="alert(1)">' })}
        onDecide={vi.fn()}
      />,
    );
    expect(container.querySelector("img")).toBeNull();
    expect(
      screen.getByText('<img src=x onerror="alert(1)">'),
    ).toBeInTheDocument();
  });

  it("shows a backend rejection", async () => {
    const onDecide = vi.fn().mockRejectedValue({
      code: "TOOL_DENIED",
      message: "The approval expired; nothing was run.",
      retryable: false,
      requestId: null,
    });
    render(<ApprovalCard request={req()} onDecide={onDecide} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Action not allowed: The approval expired",
    );
  });

  it("never uses --glass-* variables or blur on .approval-card or .memory-card in styles.css (#156)", () => {
    // Strip comments so we inspect actual selector/declaration blocks.
    const clean = css.replace(/\/\*[\s\S]*?\*\//g, "");
    const ruleRegex = /([^{}]+)\{([^{}]+)\}/g;
    const cardBlocks: { selector: string; body: string }[] = [];
    for (const match of clean.matchAll(ruleRegex)) {
      const selector = match[1].trim();
      const body = match[2].trim();
      if (
        selector.includes(".approval-card") ||
        selector.includes(".memory-card")
      ) {
        cardBlocks.push({ selector, body });
      }
    }
    expect(cardBlocks.length).toBeGreaterThan(0);
    for (const block of cardBlocks) {
      expect(block.body).not.toMatch(/--glass-/);
      const filters = [
        ...block.body.matchAll(/(?:-webkit-)?backdrop-filter\s*:\s*([^;]+)/g),
      ];
      for (const f of filters) {
        expect(f[1].trim()).toBe("none");
      }
    }
    expect(
      cardBlocks.some(
        (b) =>
          b.selector.includes(".approval-card") &&
          b.body.includes("background: var(--surface)"),
      ),
    ).toBe(true);
    expect(
      cardBlocks.some(
        (b) =>
          b.selector.includes(".memory-card") &&
          b.body.includes("background: var(--surface)"),
      ),
    ).toBe(true);
  });
});
