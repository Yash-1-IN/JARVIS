import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ChatTurn } from "../state/chatStore";
import { Chat } from "./Chat";

const turn = (over: Partial<ChatTurn>): ChatTurn => ({
  key: "k1",
  requestId: null,
  user: "hello",
  reply: "Good evening.",
  stopReason: null,
  interruptedBy: null,
  ...over,
});

describe("Chat reply", () => {
  it("marks a reply that is still arriving, and only that one", () => {
    const { container, rerender } = render(
      <Chat turns={[turn({})]} busy onSubmit={() => {}} />,
    );
    const reply = () => container.querySelector(".chat-reply");
    expect(reply()?.classList.contains("chat-reply-streaming")).toBe(true);
    // The screen-reader contract is unchanged: a polite region, busy while streaming.
    expect(reply()?.getAttribute("aria-live")).toBe("polite");
    expect(reply()?.getAttribute("aria-busy")).toBe("true");
    rerender(
      <Chat
        turns={[turn({ stopReason: "end" })]}
        busy={false}
        onSubmit={() => {}}
      />,
    );
    expect(reply()?.classList.contains("chat-reply-streaming")).toBe(false);
    expect(reply()?.getAttribute("aria-busy")).toBe("false");
  });

  it("draws the end-of-text bar with CSS, never as text a screen reader could read", () => {
    const { container } = render(
      <Chat turns={[turn({})]} busy onSubmit={() => {}} />,
    );
    expect(container.querySelector(".chat-reply")?.textContent).not.toMatch(
      /[▍|█_]/,
    );
  });
});

describe("Chat clear and scrolling", () => {
  it("Clear chat calls the handler, is disabled with nothing on screen, and says it keeps History", () => {
    const onClear = vi.fn();
    const { rerender } = render(
      <Chat turns={[]} busy={false} onSubmit={() => {}} onClear={onClear} />,
    );
    expect(screen.getByRole("button", { name: "Clear chat" })).toBeDisabled();
    rerender(
      <Chat
        turns={[turn({ stopReason: "end" })]}
        busy={false}
        onSubmit={() => {}}
        onClear={onClear}
      />,
    );
    const clear = screen.getByRole("button", { name: "Clear chat" });
    expect(clear).toHaveAttribute("title", expect.stringContaining("History"));
    fireEvent.click(clear);
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("follows the newest text at the bottom, and offers a way back after scrolling up", () => {
    const { container, rerender } = render(
      <Chat turns={[turn({})]} busy onSubmit={() => {}} />,
    );
    const el = container.querySelector(".chat-scroll") as HTMLElement;
    const box = (top: number) => {
      Object.defineProperty(el, "scrollHeight", {
        configurable: true,
        value: 1000,
      });
      Object.defineProperty(el, "clientHeight", {
        configurable: true,
        value: 400,
      });
      el.scrollTop = top;
      fireEvent.scroll(el);
    };
    // Scrolled up to read: new text does not pull the view away, and a button appears.
    box(100);
    rerender(
      <Chat turns={[turn({ reply: "more" })]} busy onSubmit={() => {}} />,
    );
    expect(el.scrollTop).toBe(100);
    fireEvent.click(screen.getByRole("button", { name: "Jump to latest" }));
    expect(el.scrollTop).toBe(1000);
    expect(
      screen.queryByRole("button", { name: "Jump to latest" }),
    ).not.toBeInTheDocument();
  });
});
