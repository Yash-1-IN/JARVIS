import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
  Channel: class {
    onmessage: (m: unknown) => void = () => {};
  },
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

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
      if (cmd === "voice_attach") return;
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
    useChatStore.setState({ turns: [], pendingInterrupts: {} });
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

describe("App errors", () => {
  beforeEach(() => {
    useSnapshotStore.setState({ snapshot: null });
    useChatStore.setState({ turns: [], pendingInterrupts: {} });
  });

  it("shows a typed error for a failed turn and Retry resends it", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    render(<App transport={t} />);
    await act(async () => {});

    t.failNextTurn("http-429");
    fireEvent.change(screen.getByLabelText("Message JARVIS"), {
      target: { value: "weather?" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send" }));
    });
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    const alert = screen.getByRole("alert");
    expect(alert).toHaveAttribute("data-code", "PROVIDER_RATE_LIMIT");
    expect(screen.getByText(/\[Stopped by an error\]/)).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    });
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByText(/You said: "weather\?"/)).toBeInTheDocument();
    vi.useRealTimers();
  });

  it("no Retry for a non-retryable auth error", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    render(<App transport={t} />);
    await act(async () => {});
    t.failNextTurn("no-key");
    fireEvent.change(screen.getByLabelText("Message JARVIS"), {
      target: { value: "hi" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send" }));
    });
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByRole("alert")).toHaveTextContent("API key problem");
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    vi.useRealTimers();
  });
});

describe("App interrupts", () => {
  beforeEach(() => {
    useSnapshotStore.setState({ snapshot: null });
    useChatStore.setState({ turns: [], pendingInterrupts: {} });
  });

  it("shows why a reply stopped when the wake word interrupts it", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    render(<App transport={t} />);
    await act(async () => {});
    fireEvent.change(screen.getByLabelText("Message JARVIS"), {
      target: { value: "long one" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send" }));
    });
    act(() => {
      vi.advanceTimersByTime(500);
      t.interrupt("wake_word");
    });
    expect(
      screen.getByText(/\[Interrupted: listening again\]/),
    ).toBeInTheDocument();
    expect(screen.getByTestId("phase")).toHaveAttribute(
      "data-phase",
      "listening",
    );
    vi.useRealTimers();
  });
});

describe("App approvals", () => {
  beforeEach(() => {
    useSnapshotStore.setState({ snapshot: null });
    useChatStore.setState({ turns: [], pendingInterrupts: {} });
  });

  it("shows the approval card and sends the digest with the decision", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    const spy = vi.spyOn(t, "resolveApproval");
    render(<App transport={t} />);
    await act(async () => {});
    act(() => {
      t.runScript("approval");
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByTestId("approval-card")).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    });
    expect(spy).toHaveBeenCalledWith({
      approvalId: "mock-appr-1",
      argsDigest: "mock-digest-0000",
      decision: "deny",
    });
    expect(screen.queryByTestId("approval-card")).toBeNull();
    vi.useRealTimers();
  });
});

describe("App panels (issue #37)", () => {
  beforeEach(() => {
    useSnapshotStore.setState({ snapshot: null });
    useChatStore.setState({ turns: [], pendingInterrupts: {} });
  });

  it("opens one panel at a time beside the chat, with focus moved in and back", async () => {
    render(<App transport={new MockTransport()} />);
    await screen.findByTestId("phase");
    const settingsBtn = screen.getByRole("button", { name: "Settings" });
    const activityBtn = screen.getByRole("button", { name: "Activity" });
    expect(settingsBtn).toHaveAttribute("aria-pressed", "false");

    await act(async () => fireEvent.click(settingsBtn));
    const panel = screen.getByRole("region", { name: "Settings" });
    expect(settingsBtn).toHaveAttribute("aria-pressed", "true");
    expect(settingsBtn).toHaveAttribute("aria-controls", panel.id);
    expect(screen.getByRole("heading", { name: "Settings" })).toHaveFocus();
    // The chat stays on screen.
    expect(screen.getByLabelText("Message JARVIS")).toBeInTheDocument();

    // Opening Activity replaces Settings.
    await act(async () => fireEvent.click(activityBtn));
    expect(screen.queryByRole("region", { name: "Settings" })).toBeNull();
    expect(
      screen.getByRole("region", { name: "Activity" }),
    ).toBeInTheDocument();
    expect(activityBtn).toHaveAttribute("aria-pressed", "true");
    expect(settingsBtn).toHaveAttribute("aria-pressed", "false");

    // Clicking the open one again closes it.
    await act(async () => fireEvent.click(activityBtn));
    expect(screen.queryByRole("region", { name: "Activity" })).toBeNull();

    // Close returns focus to the button that opened the panel.
    await act(async () => fireEvent.click(settingsBtn));
    await act(async () =>
      fireEvent.click(
        within(screen.getByRole("region", { name: "Settings" })).getByRole(
          "button",
          { name: /Close/ },
        ),
      ),
    );
    expect(screen.queryByRole("region", { name: "Settings" })).toBeNull();
    expect(settingsBtn).toHaveFocus();
  });

  it("keeps the top bar and the side panel's offset in one sticky block, with approvals inside it", async () => {
    const { container } = render(<App transport={new MockTransport()} />);
    await screen.findByTestId("phase");
    const top = container.querySelector(".app-top");
    expect(top).not.toBeNull();
    // The header stays visible with the approval cards under it (CSS: position sticky).
    expect(top?.querySelector("header.app-header")).not.toBeNull();
    expect(
      within(top as HTMLElement).getByRole("navigation", { name: "Panels" }),
    ).toBeInTheDocument();
    // The conversation is not inside it, so it scrolls underneath.
    expect(top?.querySelector(".chat")).toBeNull();
  });

  it("puts the page back where it was when a panel closes, and does not scroll to the button", async () => {
    // jsdom does not scroll; give the page a scroll position that we can set and read.
    let top = 0;
    Object.defineProperty(document.documentElement, "scrollTop", {
      configurable: true,
      get: () => top,
      set: (v: number) => {
        top = v;
      },
    });
    const focus = vi.spyOn(HTMLElement.prototype, "focus");
    try {
      render(<App transport={new MockTransport()} />);
      await screen.findByTestId("phase");
      top = 640; // scrolled down the conversation
      await act(async () =>
        fireEvent.click(screen.getByRole("button", { name: "Settings" })),
      );
      // On a narrow window the conversation is hidden and the page loses its place.
      top = 0;
      await act(async () =>
        fireEvent.click(
          within(screen.getByRole("region", { name: "Settings" })).getByRole(
            "button",
            { name: /Close/ },
          ),
        ),
      );
      expect(top).toBe(640);
      // Neither moving focus into the panel nor back to its button may scroll the page.
      const calls = focus.mock.calls.filter(([opts]) => opts !== undefined);
      expect(calls.length).toBeGreaterThanOrEqual(2);
      for (const [opts] of calls) {
        expect(opts).toEqual({ preventScroll: true });
      }
      // Switching between panels keeps the first saved position, not the one in between.
      top = 300;
      await act(async () =>
        fireEvent.click(screen.getByRole("button", { name: "Activity" })),
      );
      top = 0;
      await act(async () =>
        fireEvent.click(screen.getByRole("button", { name: "Memory" })),
      );
      top = 0;
      await act(async () =>
        fireEvent.click(screen.getByRole("button", { name: "Memory" })),
      );
      expect(top).toBe(300);
    } finally {
      focus.mockRestore();
      delete (document.documentElement as unknown as Record<string, unknown>)
        .scrollTop;
    }
  });

  it("Esc inside a panel closes it and does not cancel the reply; outside it still cancels", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    const cancel = vi.spyOn(t, "cancel");
    render(<App transport={t} />);
    await act(async () => {});
    fireEvent.change(screen.getByLabelText("Message JARVIS"), {
      target: { value: "long one" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Send" }));
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });

    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Activity" })),
    );
    const heading = screen.getByRole("heading", { name: "Activity" });
    await act(async () => {
      fireEvent.keyDown(heading, { key: "Escape" });
    });
    expect(screen.queryByRole("region", { name: "Activity" })).toBeNull();
    expect(cancel).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Activity" })).toHaveFocus();

    await act(async () => {
      fireEvent.keyDown(window, { key: "Escape" });
    });
    expect(cancel).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("keeps the approval card on screen and reachable while a panel is open", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    render(<App transport={t} />);
    await act(async () => {});
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Settings" })),
    );
    act(() => {
      t.runScript("approval");
      vi.advanceTimersByTime(1000);
    });
    const card = screen.getByTestId("approval-card");
    expect(
      screen.getByRole("region", { name: "Settings" }),
    ).not.toContainElement(card);
    expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
    vi.useRealTimers();
  });
});

describe("App push to talk", () => {
  beforeEach(() => {
    useSnapshotStore.setState({ snapshot: null });
    useChatStore.setState({ turns: [], pendingInterrupts: {} });
  });

  it("holding the button listens, and releasing puts the heard words and the reply in the conversation", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    render(<App transport={t} />);
    await act(async () => {});
    const talk = screen.getByRole("button", { name: /talk/i });

    await act(async () => {
      fireEvent.pointerDown(talk);
    });
    expect(screen.getByTestId("phase")).toHaveAttribute(
      "data-phase",
      "listening",
    );
    // Listening at once, but the device is still opening: not capturing yet.
    expect(screen.getByTestId("mic")).toHaveAttribute("data-mic", "off");
    expect(talk).toHaveTextContent("Starting microphone…");
    await act(async () => {
      vi.advanceTimersByTime(500);
    });
    expect(screen.getByTestId("mic")).toHaveAttribute("data-mic", "capturing");
    expect(talk).toHaveTextContent("Speak now");

    await act(async () => {
      fireEvent.pointerUp(talk);
    });
    await act(async () => {
      vi.advanceTimersByTime(5000);
    });
    expect(
      screen.getAllByText(/MOCK: nothing was heard/).length,
    ).toBeGreaterThan(0);
    expect(screen.getByText(/MOCK reply, not a model/)).toBeInTheDocument();
    expect(screen.getByTestId("mic")).toHaveAttribute("data-mic", "off");
    vi.useRealTimers();
  });

  it("a quick tap keeps listening and ends by itself when speech ends", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    render(<App transport={t} />);
    await act(async () => {});
    const talk = screen.getByRole("button", { name: /talk/i });

    await act(async () => {
      fireEvent.pointerDown(talk);
    });
    await act(async () => {
      vi.advanceTimersByTime(50);
      fireEvent.pointerUp(talk);
    });
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    // Still listening: nothing was sent on the release.
    expect(screen.getByTestId("phase")).toHaveAttribute(
      "data-phase",
      "listening",
    );
    expect(talk).toHaveTextContent("press to send");

    await act(async () => {
      vi.advanceTimersByTime(8000);
    });
    expect(
      screen.getAllByText(/MOCK: nothing was heard/).length,
    ).toBeGreaterThan(0);
    expect(talk).toHaveTextContent("Hold or tap to talk");
    vi.useRealTimers();
  });

  it("cannot be used while the microphone is muted", async () => {
    const t = new MockTransport();
    render(<App transport={t} />);
    await act(async () => {});
    await act(async () => {
      fireEvent.keyDown(window, { key: "M", ctrlKey: true, shiftKey: true });
    });
    expect(screen.getByRole("button", { name: /talk/i })).toBeDisabled();
  });
});
