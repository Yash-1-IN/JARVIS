import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "../App";
import { useChatStore } from "../state/chatStore";
import { useSnapshotStore } from "../state/snapshotStore";
import { MockTransport } from "../transport/mock/MockTransport";
import type { ConversationSummary } from "../types/generated/ConversationSummary";

async function say(text: string) {
  fireEvent.change(screen.getByLabelText("Message JARVIS"), {
    target: { value: text },
  });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
  });
  await act(async () => {
    vi.advanceTimersByTime(60_000);
  });
}

async function start(t = new MockTransport()) {
  render(<App transport={t} />);
  await act(async () => {});
  return t;
}

async function openHistory() {
  await act(async () =>
    fireEvent.click(screen.getByRole("button", { name: "History" })),
  );
  return screen.getByRole("region", { name: "History" });
}

describe("History panel (M5 UI on the mock)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useSnapshotStore.setState({ snapshot: null });
    useChatStore.setState({ turns: [], pendingInterrupts: {} });
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("lists a conversation and shows its messages as plain text", async () => {
    await start();
    await say("**hello** <b>there</b>");
    const panel = await openHistory();
    const list = within(panel).getByRole("list", { name: "Conversations" });
    const open = within(list).getByRole("button", {
      name: /\*\*hello\*\* <b>there<\/b>.*2 messages/,
    });
    await act(async () => fireEvent.click(open));
    const msgs = within(panel).getByRole("list", { name: "Messages" });
    expect(within(msgs).getAllByRole("listitem")).toHaveLength(2);
    // Stored text is never rendered as markup.
    expect(msgs.querySelector("b, strong")).toBeNull();
    expect(
      within(msgs).getByText("**hello** <b>there</b>"),
    ).toBeInTheDocument();
    expect(within(msgs).getByText("You")).toBeInTheDocument();
    expect(within(msgs).getByText("JARVIS")).toBeInTheDocument();
    await act(async () =>
      fireEvent.click(
        within(panel).getByRole("button", { name: /All conversations/ }),
      ),
    );
    expect(
      within(panel).getByRole("list", { name: "Conversations" }),
    ).toBeInTheDocument();
  });

  it("keeps a neutral line, not the text, for 'remember that'", async () => {
    await start();
    await say("remember that zebrapassphrase opens the shed");
    const panel = await openHistory();
    expect(panel.textContent).not.toContain("zebrapassphrase");
    expect(panel).toHaveTextContent("Asked JARVIS to remember something");
  });

  it("asks before a shorter retention; a longer one saves at once", async () => {
    const t = await start();
    const set = vi.spyOn(t, "setHistorySettings");
    await say("hi");
    const panel = await openHistory();
    const select = within(panel).getByLabelText("Keep history for");
    expect(select).toHaveValue("days30");

    await act(async () =>
      fireEvent.change(select, { target: { value: "forever" } }),
    );
    expect(set).toHaveBeenLastCalledWith({ retention: "forever" });

    await act(async () =>
      fireEvent.change(select, { target: { value: "off" } }),
    );
    expect(set).toHaveBeenCalledTimes(1);
    expect(
      within(panel).getByText(/deletes every saved conversation now/),
    ).toBeVisible();
    await act(async () =>
      fireEvent.click(within(panel).getByRole("button", { name: "Cancel" })),
    );
    expect(set).toHaveBeenCalledTimes(1);

    await act(async () =>
      fireEvent.change(select, { target: { value: "off" } }),
    );
    await act(async () =>
      fireEvent.click(
        within(panel).getByRole("button", { name: "Change and delete" }),
      ),
    );
    expect(set).toHaveBeenLastCalledWith({ retention: "off" });
    expect(within(panel).getByTestId("history-empty")).toBeInTheDocument();
  });

  it("deletes all history only after confirming, and memories stay", async () => {
    const t = await start();
    await say("remember that I like tea");
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Keep" })),
    );
    await say("hello");
    const panel = await openHistory();
    const del = vi.spyOn(t, "deleteAllHistory");
    await act(async () =>
      fireEvent.click(
        within(panel).getByRole("button", { name: "Delete all history" }),
      ),
    );
    expect(del).not.toHaveBeenCalled();
    await act(async () =>
      fireEvent.click(
        within(panel).getByRole("button", { name: "Delete all" }),
      ),
    );
    expect(del).toHaveBeenCalledTimes(1);
    expect(within(panel).getByTestId("history-empty")).toBeInTheDocument();
    expect(await t.listMemories()).toHaveLength(1);
  });

  it("exports memories and history as a downloaded JSON file", async () => {
    const blobs: Blob[] = [];
    vi.stubGlobal("URL", {
      ...URL,
      createObjectURL: (b: Blob) => {
        blobs.push(b);
        return "blob:mock";
      },
      revokeObjectURL: () => {},
    });
    const clicked: string[] = [];
    const click = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(function (this: HTMLAnchorElement) {
        clicked.push(this.download);
      });
    await start();
    await say("hello");
    const panel = await openHistory();
    await act(async () =>
      fireEvent.click(
        within(panel).getByRole("button", {
          name: "Export memories and history",
        }),
      ),
    );
    expect(clicked[0]).toMatch(/^jarvis-export-\d{4}-\d{2}-\d{2}\.json$/);
    const doc = JSON.parse(await blobs[0].text());
    expect(doc).toMatchObject({ format: "jarvis-export", version: 1 });
    expect(doc.history).toHaveLength(1);
    expect(within(panel).getByRole("status")).toHaveTextContent("Export saved");
    click.mockRestore();
  });

  it("pages older conversations", async () => {
    const t = await start();
    const page = (n: number, from: number): ConversationSummary[] =>
      Array.from({ length: n }, (_, i) => ({
        id: `c${from + i}`,
        startedAt: 0,
        endedAt: 0,
        title: `Chat ${from + i}`,
        messageCount: 1,
      }));
    const list = vi
      .spyOn(t, "listConversations")
      .mockResolvedValueOnce(page(30, 0))
      .mockResolvedValueOnce(page(5, 30));
    const panel = await openHistory();
    await act(async () =>
      fireEvent.click(
        within(panel).getByRole("button", { name: "Show older" }),
      ),
    );
    expect(list).toHaveBeenLastCalledWith(30, "c29");
    expect(
      within(
        within(panel).getByRole("list", { name: "Conversations" }),
      ).getAllByRole("listitem"),
    ).toHaveLength(35);
    expect(
      within(panel).queryByRole("button", { name: "Show older" }),
    ).toBeNull();
  });
});
