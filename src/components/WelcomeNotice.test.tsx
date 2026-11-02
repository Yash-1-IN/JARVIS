import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MockTransport } from "../transport/mock/MockTransport";
import { WelcomeNotice } from "./WelcomeNotice";

async function show(t = new MockTransport()) {
  render(<WelcomeNotice transport={t} />);
  await act(async () => {});
  return t;
}

describe("WelcomeNotice (issue #45)", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => vi.restoreAllMocks());

  it("shows once on first launch, with two equal choices and nothing focused", async () => {
    await show();
    const notice = screen.getByTestId("welcome");
    const keep = screen.getByRole("button", { name: "Keep asking" });
    const stop = screen.getByRole("button", {
      name: "Stop asking for routine actions",
    });
    expect(notice).not.toContainElement(document.activeElement as HTMLElement);
    expect(keep.className).toBe(stop.className);
    // It says plainly what changes and what does not.
    expect(notice).toHaveTextContent("sends its contents to the AI provider");
    expect(notice).toHaveTextContent("always for bigger actions");
    expect(notice).toHaveTextContent("Settings → Permissions");
  });

  it("Stop asking turns on autoAllowMedium and is not shown again", async () => {
    const t = await show();
    const update = vi.spyOn(t, "updateSettings");
    await act(async () =>
      fireEvent.click(
        screen.getByRole("button", { name: "Stop asking for routine actions" }),
      ),
    );
    expect(update).toHaveBeenCalledWith({ autoAllowMedium: true });
    expect(screen.queryByTestId("welcome")).toBeNull();
    expect((await t.getSettings()).autoAllowMedium).toBe(true);
  });

  it("Keep asking, Close and Esc change nothing and are remembered", async () => {
    for (const how of ["keep", "close", "esc"] as const) {
      localStorage.clear();
      const t = new MockTransport();
      const update = vi.spyOn(t, "updateSettings");
      const { unmount } = render(<WelcomeNotice transport={t} />);
      await act(async () => {});
      if (how === "keep") {
        fireEvent.click(screen.getByRole("button", { name: "Keep asking" }));
      } else if (how === "close") {
        fireEvent.click(screen.getByRole("button", { name: /Close/ }));
      } else {
        fireEvent.keyDown(screen.getByRole("button", { name: "Keep asking" }), {
          key: "Escape",
        });
      }
      expect(screen.queryByTestId("welcome"), how).toBeNull();
      expect(update, how).not.toHaveBeenCalled();
      unmount();
      // Second launch: not shown.
      const again = render(<WelcomeNotice transport={t} />);
      await act(async () => {});
      expect(screen.queryByTestId("welcome"), how).toBeNull();
      again.unmount();
    }
  });

  it("is not shown when routine actions already run without asking", async () => {
    const t = new MockTransport();
    await t.updateSettings({ autoAllowMedium: true });
    await show(t);
    expect(screen.queryByTestId("welcome")).toBeNull();
  });

  it("unreadable storage shows it again without crashing", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    await show();
    fireEvent.click(screen.getByRole("button", { name: "Keep asking" }));
    expect(screen.queryByTestId("welcome")).toBeNull();
  });

  it("shows why if switching it on fails, and stays open", async () => {
    const t = new MockTransport();
    vi.spyOn(t, "updateSettings").mockRejectedValue({
      code: "INTERNAL",
      message: "settings file is read-only",
      retryable: false,
      requestId: null,
    });
    await show(t);
    await act(async () =>
      fireEvent.click(
        screen.getByRole("button", { name: "Stop asking for routine actions" }),
      ),
    );
    expect(screen.getByRole("alert")).toHaveTextContent("read-only");
    expect(screen.getByTestId("welcome")).toBeInTheDocument();
  });
});
