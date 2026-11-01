import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "../App";
import { useChatStore } from "../state/chatStore";
import { useSnapshotStore } from "../state/snapshotStore";
import { MockTransport } from "../transport/mock/MockTransport";

async function say(text: string) {
  fireEvent.change(screen.getByLabelText("Message JARVIS"), {
    target: { value: text },
  });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
  });
  await act(async () => {
    vi.advanceTimersByTime(500);
  });
}

async function start(t = new MockTransport()) {
  render(<App transport={t} />);
  await act(async () => {});
  return t;
}

async function openMemory() {
  await act(async () =>
    fireEvent.click(screen.getByRole("button", { name: "Memory" })),
  );
  return screen.getByRole("region", { name: "Memory" });
}

describe("Memory (M5 UI on the mock)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    useSnapshotStore.setState({ snapshot: null });
    useChatStore.setState({ turns: [], pendingInterrupts: {} });
  });
  afterEach(() => vi.useRealTimers());

  it("'remember that' shows the exact text on a card; Keep stores it", async () => {
    const t = await start();
    const accept = vi.spyOn(t, "acceptMemory");
    await say("Remember that my cat is called Miso.");
    const card = screen.getByTestId("memory-card");
    expect(
      within(card).getByText("my cat is called Miso."),
    ).toBeInTheDocument();
    // The reply does not repeat the text; the card does.
    expect(screen.getByText(/Confirm it on the card/)).toBeInTheDocument();
    // Nothing on the card takes focus by itself.
    expect(card).not.toContainElement(document.activeElement as HTMLElement);

    await act(async () =>
      fireEvent.click(within(card).getByRole("button", { name: "Keep" })),
    );
    expect(accept).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("memory-card")).toBeNull();

    const panel = await openMemory();
    const kept = within(panel).getByRole("list", { name: "Kept" });
    expect(
      within(kept).getByText("my cat is called Miso."),
    ).toBeInTheDocument();
  });

  it("Don't keep deletes the proposal; nothing is stored", async () => {
    const t = await start();
    await say("remember: the door code");
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Don't keep" })),
    );
    expect(screen.queryByTestId("memory-card")).toBeNull();
    expect(await t.listMemories()).toEqual([]);
  });

  it("Edit on the card changes the text before keeping it", async () => {
    const t = await start();
    await say("remember that I like tea");
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    fireEvent.change(screen.getByLabelText("Memory text"), {
      target: { value: "I like green tea" },
    });
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Save and keep" })),
    );
    expect(screen.queryByTestId("memory-card")).toBeNull();
    const [m] = await t.listMemories();
    expect(m).toMatchObject({ content: "I like green tea", status: "active" });
  });

  it("a proposal from the model shows the same card", async () => {
    const t = await start();
    act(() => t.simulateMemoryProposal());
    expect(screen.getByTestId("memory-card")).toHaveTextContent(
      "Prefers short answers before 9 am.",
    );
  });

  it("the panel edits within 500 code points and deletes only after confirming", async () => {
    const t = await start();
    const m = (await t.listMemories()).length;
    expect(m).toBe(0);
    await say("remember that I live in Pune");
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Keep" })),
    );
    const panel = await openMemory();
    const item = within(panel).getByTestId("memory-item");

    fireEvent.click(within(item).getByRole("button", { name: "Edit" }));
    const box = within(item).getByLabelText("Memory text");
    fireEvent.change(box, { target: { value: "😀".repeat(501) } });
    expect(within(item).getByText(/501 \/ 500/)).toBeInTheDocument();
    expect(within(item).getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.change(box, { target: { value: "😀".repeat(500) } });
    expect(within(item).getByRole("button", { name: "Save" })).toBeEnabled();
    fireEvent.change(box, { target: { value: "I live in Mumbai" } });
    await act(async () =>
      fireEvent.click(within(item).getByRole("button", { name: "Save" })),
    );
    expect(within(panel).getByText("I live in Mumbai")).toBeInTheDocument();

    fireEvent.click(within(panel).getByRole("button", { name: "Delete" }));
    expect(await t.listMemories()).toHaveLength(1);
    expect(
      within(panel).getByText(/Delete this memory for good\?/),
    ).toBeVisible();
    await act(async () =>
      fireEvent.click(within(panel).getByRole("button", { name: "Delete" })),
    );
    expect(await t.listMemories()).toEqual([]);
    expect(within(panel).getByTestId("memory-empty")).toBeInTheDocument();
  });

  it("a memory deleted elsewhere (NOT_FOUND) is dropped from the card and the list", async () => {
    const t = await start();
    await say("remember that X is Y");
    const [m] = await t.listMemories();
    await t.deleteMemory(m.id); // e.g. another window
    await act(async () =>
      fireEvent.click(screen.getByRole("button", { name: "Keep" })),
    );
    expect(screen.queryByTestId("memory-card")).toBeNull();
    const panel = await openMemory();
    expect(within(panel).getByTestId("memory-empty")).toBeInTheDocument();
  });
});
