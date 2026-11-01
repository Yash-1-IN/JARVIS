import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import App from "./App";
import { useChatStore } from "./state/chatStore";
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

describe("App keyboard controls", () => {
  beforeEach(() => useSnapshotStore.setState({ snapshot: null }));

  it("Ctrl+Shift+M toggles mute and Esc cancels the active request", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    render(<App transport={t} />);
    await act(async () => {});

    await act(async () => {
      fireEvent.keyDown(window, { key: "M", ctrlKey: true, shiftKey: true });
    });
    expect(screen.getByTestId("mic")).toHaveAttribute("data-mic", "muted");
    expect(screen.getByRole("button", { name: /Unmute/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    act(() => {
      t.runScript("wake-listen-think-speak");
      vi.advanceTimersByTime(800);
    });
    expect(screen.getByTestId("phase")).toHaveAttribute(
      "data-phase",
      "listening",
    );

    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    expect(screen.getByTestId("phase")).toHaveAttribute("data-phase", "idle");
    expect(screen.getByRole("button", { name: /Cancel/ })).toBeDisabled();
    vi.useRealTimers();
  });

  it("shows an alert when a command fails on the real backend", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "ping")
        return { pong: true, protocolVersion: 1, appVersion: "t" };
      if (cmd === "get_snapshot")
        return {
          seq: 0,
          mic: "standby",
          phase: "idle",
          activeRequestId: null,
          network: "unknown",
          cloudInUse: false,
          pendingApprovals: [],
          lastError: null,
        };
      throw "command set_mute not found";
    });
    render(<App transport={new TauriTransport()} />);
    await screen.findByTestId("mic");
    await act(async () => {
      fireEvent.keyDown(window, { key: "m", metaKey: true, shiftKey: true });
    });
    expect(screen.getByRole("alert")).toHaveTextContent("set_mute not found");
  });
});

describe("App chat", () => {
  beforeEach(() => {
    useSnapshotStore.setState({ snapshot: null });
    useChatStore.setState({ turns: [] });
  });

  it("sends a message, streams the mock reply, and Esc cancels mid-stream", async () => {
    vi.useFakeTimers();
    render(<App transport={new MockTransport()} />);
    await act(async () => {});

    const input = screen.getByLabelText("Message JARVIS");
    fireEvent.change(input, { target: { value: "first" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send" }));
    });
    expect(screen.getByTestId("phase")).toHaveAttribute(
      "data-phase",
      "thinking",
    );
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();

    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(screen.getByText(/You said: "first"/)).toBeInTheDocument();

    fireEvent.change(input, { target: { value: "second" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send" }));
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    expect(screen.getByText(/\[Cancelled\]/)).toBeInTheDocument();
    expect(screen.getByTestId("phase")).toHaveAttribute("data-phase", "idle");
    vi.useRealTimers();
  });
});
