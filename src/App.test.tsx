import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import App from "./App";
import { useSnapshotStore } from "./state/snapshotStore";
import { MockTransport } from "./transport/mock/MockTransport";
import { TauriTransport } from "./transport/tauri";

describe("App", () => {
  beforeEach(() => useSnapshotStore.setState({ snapshot: null }));

  it("shows the MOCK badge and scenario buttons with MockTransport", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    render(<App transport={t} />);
    await act(async () => {});
    expect(screen.getByText("MOCK")).toBeInTheDocument();

    act(() => {
      screen.getByRole("button", { name: "wake-listen-think-speak" }).click();
      vi.advanceTimersByTime(800);
    });
    expect(screen.getByTestId("phase")).toHaveAttribute(
      "data-phase",
      "listening",
    );
    vi.useRealTimers();
  });

  it("has no MOCK badge with TauriTransport", () => {
    render(<App transport={new TauriTransport()} />);
    expect(screen.queryByText("MOCK")).not.toBeInTheDocument();
  });
});
