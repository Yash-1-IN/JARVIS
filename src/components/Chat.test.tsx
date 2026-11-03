import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
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
