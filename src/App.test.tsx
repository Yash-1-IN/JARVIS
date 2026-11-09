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
import { useGlassPref, usePanelSplit } from "./state/layoutPref";
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

  it("keeps the top bar and the approval cards in one block above the work area", async () => {
    const { container } = render(<App transport={new MockTransport()} />);
    await screen.findByTestId("phase");
    const top = container.querySelector(".app-top");
    expect(top).not.toBeNull();
    // The header stays on top with the approval cards under it (the work area scrolls inside itself).
    expect(top?.querySelector("header.app-header")).not.toBeNull();
    expect(
      within(top as HTMLElement).getByRole("navigation", { name: "Panels" }),
    ).toBeInTheDocument();
    // The conversation is not inside it.
    expect(top?.querySelector(".chat")).toBeNull();
  });

  it("opening and closing a panel never hides or rebuilds the conversation, and never scrolls to the button", async () => {
    const focus = vi.spyOn(HTMLElement.prototype, "focus");
    try {
      const { container } = render(<App transport={new MockTransport()} />);
      await screen.findByTestId("phase");
      const log = container.querySelector(".chat-scroll") as HTMLElement;
      expect(log).not.toBeNull();
      log.scrollTop = 640; // scrolled up the conversation
      await act(async () =>
        fireEvent.click(screen.getByRole("button", { name: "Settings" })),
      );
      // The same scroll area is still there, beside the open panel, with the same position.
      expect(container.querySelector(".chat-scroll")).toBe(log);
      expect(screen.getByRole("region", { name: "Settings" })).toBeVisible();
      expect(log.scrollTop).toBe(640);
      await act(async () =>
        fireEvent.click(
          within(screen.getByRole("region", { name: "Settings" })).getByRole(
            "button",
            { name: /Close/ },
          ),
        ),
      );
      expect(container.querySelector(".chat-scroll")).toBe(log);
      expect(log.scrollTop).toBe(640);
      // Neither moving focus into the panel nor back to its button may scroll anything.
      const calls = focus.mock.calls.filter(([opts]) => opts !== undefined);
      expect(calls.length).toBeGreaterThanOrEqual(2);
      for (const [opts] of calls) {
        expect(opts).toEqual({ preventScroll: true });
      }
    } finally {
      focus.mockRestore();
    }
  });

  it("opens a panel as a pop-up window by default: a modal dialog over a page that cannot be reached, closed by Esc, Close or a click outside", async () => {
    usePanelSplit.setState({ split: "popup" });
    try {
      const { container } = render(<App transport={new MockTransport()} />);
      await screen.findByTestId("phase");
      const settingsBtn = screen.getByRole("button", { name: "Settings" });
      const rail = container.querySelector("aside.rail");
      const chat = container.querySelector("section.chat");
      await act(async () => fireEvent.click(settingsBtn));
      const dialog = screen.getByRole("dialog", { name: "Settings" });
      expect(dialog).toHaveAttribute("aria-modal", "true");
      expect(dialog.closest(".modal-backdrop")).not.toBeNull();
      // The page behind is dimmed and cannot be reached, and nothing in it moved or was rebuilt.
      expect(container.querySelector(".app-top")).toHaveAttribute("inert");
      expect(container.querySelector(".app-main")).toHaveAttribute("inert");
      expect(container.querySelector("aside.rail")).toBe(rail);
      expect(container.querySelector("section.chat")).toBe(chat);
      // A click inside does nothing; a click on the dimmed area closes it and gives the focus back.
      fireEvent.mouseDown(dialog);
      expect(
        screen.getByRole("dialog", { name: "Settings" }),
      ).toBeInTheDocument();
      await act(async () =>
        fireEvent.mouseDown(container.querySelector(".modal-backdrop")!),
      );
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(settingsBtn).toHaveFocus();
      expect(container.querySelector(".app-main")).not.toHaveAttribute("inert");
      // Esc inside it closes it too, and does not cancel anything.
      await act(async () => fireEvent.click(settingsBtn));
      await act(async () =>
        fireEvent.keyDown(screen.getByRole("dialog", { name: "Settings" }), {
          key: "Escape",
        }),
      );
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(settingsBtn).toHaveFocus();
    } finally {
      usePanelSplit.setState({ split: "rail" });
    }
  });

  it("hides the pop-up while an approval card waits, so the card is never behind it, and brings it back after", async () => {
    vi.useFakeTimers();
    usePanelSplit.setState({ split: "popup" });
    try {
      const t = new MockTransport();
      const { container } = render(<App transport={t} />);
      await act(async () => {});
      await act(async () =>
        fireEvent.click(screen.getByRole("button", { name: "Settings" })),
      );
      expect(screen.getByRole("dialog", { name: "Settings" })).toBeVisible();
      expect(container.querySelector(".app-top")).toHaveAttribute("inert");
      // An approval arrives: the pop-up is hidden (still mounted) and the page can be reached.
      act(() => {
        t.runScript("approval");
        vi.advanceTimersByTime(5000);
      });
      expect(container.querySelector(".approvals")).not.toBeNull();
      expect(container.querySelector(".modal-backdrop")).toHaveAttribute(
        "hidden",
      );
      expect(container.querySelector(".app-top")).not.toHaveAttribute("inert");
      expect(container.querySelector(".app-main")).not.toHaveAttribute("inert");
    } finally {
      usePanelSplit.setState({ split: "rail" });
      vi.useRealTimers();
    }
  });

  it("keeps Tab inside the pop-up and moves between the settings sections with the arrow keys", async () => {
    usePanelSplit.setState({ split: "popup" });
    try {
      render(<App transport={new MockTransport()} />);
      await screen.findByTestId("phase");
      await act(async () =>
        fireEvent.click(screen.getByRole("button", { name: "Settings" })),
      );
      const dialog = await screen.findByRole("dialog", { name: "Settings" });
      const close = within(dialog).getByRole("button", { name: /Close/ });
      // Shift+Tab from the first control goes to the last one inside, not to the page.
      close.focus();
      await act(async () =>
        fireEvent.keyDown(close, { key: "Tab", shiftKey: true }),
      );
      expect(dialog.contains(document.activeElement)).toBe(true);
      expect(document.activeElement).not.toBe(close);
      // Tab from the last control goes back to the first.
      const last = document.activeElement as HTMLElement;
      await act(async () => fireEvent.keyDown(last, { key: "Tab" }));
      expect(dialog.contains(document.activeElement)).toBe(true);
      // The sections are a vertical list: Down and Up move between them.
      const tabs = within(dialog).getAllByRole("tab");
      tabs[0].focus();
      await act(async () => fireEvent.keyDown(tabs[0], { key: "ArrowDown" }));
      expect(tabs[1]).toHaveAttribute("aria-selected", "true");
      await act(async () => fireEvent.keyDown(tabs[1], { key: "ArrowUp" }));
      expect(tabs[0]).toHaveAttribute("aria-selected", "true");
    } finally {
      usePanelSplit.setState({ split: "rail" });
    }
  });

  it.each([
    ["rail", ".rail-col", "aside.rail"],
    ["stage", ".stage", "section.chat"],
    ["both", ".app-main", "main.workspace"],
  ] as const)(
    "opens a panel where the person chose (%s) and leaves the rest where it was",
    async (split, parentSel, nextSel) => {
      usePanelSplit.setState({ split });
      try {
        const { container } = render(<App transport={new MockTransport()} />);
        await screen.findByTestId("phase");
        const rail = container.querySelector("aside.rail") as HTMLElement;
        const chat = container.querySelector("section.chat") as HTMLElement;
        // Closed: the talk column and the conversation are side by side.
        expect(rail.querySelector(".push-to-talk")).not.toBeNull();
        await act(async () =>
          fireEvent.click(screen.getByRole("button", { name: "Settings" })),
        );
        const panel = screen.getByRole("region", { name: "Settings" });
        const parent = container.querySelector(parentSel) as HTMLElement;
        // The panel is in the chosen place, above what it shares the space with.
        expect(panel.parentElement).toBe(parent);
        expect(panel.nextElementSibling).toBe(container.querySelector(nextSel));
        // Nothing was rebuilt: the same talk button and conversation are still there.
        expect(container.querySelector("aside.rail")).toBe(rail);
        expect(container.querySelector("section.chat")).toBe(chat);
        expect(rail.querySelector(".push-to-talk")).not.toBeNull();
      } finally {
        usePanelSplit.setState({ split: "rail" });
      }
    },
  );

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

  it("sets data-glass on the app root from the Display glass preference", async () => {
    useGlassPref.setState({ glass: "auto" });
    try {
      const { container } = render(<App transport={new MockTransport()} />);
      await screen.findByTestId("phase");
      const app = container.querySelector(".app");
      expect(app).toHaveAttribute("data-glass", "auto");
      act(() => {
        useGlassPref.getState().setGlass("on");
      });
      expect(app).toHaveAttribute("data-glass", "on");
      act(() => {
        useGlassPref.getState().setGlass("off");
      });
      expect(app).toHaveAttribute("data-glass", "off");
    } finally {
      useGlassPref.setState({ glass: "auto" });
    }
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
