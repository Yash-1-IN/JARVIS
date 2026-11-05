import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PushToTalk } from "./PushToTalk";

function setup(props: Partial<React.ComponentProps<typeof PushToTalk>> = {}) {
  const onStart = vi.fn(() => Promise.resolve());
  const onStop = vi.fn();
  const view = render(
    <PushToTalk
      unavailable={false}
      busy={false}
      micOpen={false}
      onStart={onStart}
      onStop={onStop}
      {...props}
    />,
  );
  return { onStart, onStop, ...view };
}

const button = () =>
  screen.getByRole("button", { name: /talk|starting microphone|speak now/i });

describe("PushToTalk", () => {
  it("starts on pointer down and stops once on release", () => {
    const { onStart, onStop } = setup();
    fireEvent.pointerDown(button());
    expect(onStart).toHaveBeenCalledTimes(1);
    expect(button()).toHaveAttribute("aria-pressed", "true");
    fireEvent.pointerUp(button());
    fireEvent.pointerUp(button());
    expect(onStop).toHaveBeenCalledTimes(1);
    expect(button()).toHaveAttribute("aria-pressed", "false");
  });

  it("works with the space bar and ignores key repeat", () => {
    const { onStart, onStop } = setup();
    fireEvent.keyDown(button(), { key: " " });
    fireEvent.keyDown(button(), { key: " ", repeat: true });
    expect(onStart).toHaveBeenCalledTimes(1);
    fireEvent.keyUp(button(), { key: " " });
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["a cancelled pointer", () => fireEvent.pointerCancel(button())],
    ["focus leaving the button", () => fireEvent.blur(button())],
    [
      "the window being hidden",
      () => {
        Object.defineProperty(document, "hidden", {
          configurable: true,
          get: () => true,
        });
        document.dispatchEvent(new Event("visibilitychange"));
        Object.defineProperty(document, "hidden", {
          configurable: true,
          get: () => false,
        });
      },
    ],
  ])("stops the capture on %s", (_name, end) => {
    const { onStop } = setup();
    fireEvent.pointerDown(button());
    act(() => {
      end();
    });
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("goes back to idle when the start is refused", async () => {
    const onStart = vi.fn(() => Promise.reject(new Error("refused")));
    const { onStop } = setup({ onStart });
    await act(async () => {
      fireEvent.pointerDown(button());
    });
    expect(button()).toHaveAttribute("aria-pressed", "false");
    fireEvent.pointerUp(button());
    expect(onStop).not.toHaveBeenCalled();
  });

  it("is disabled while muted or while another request runs, and muting ends a press", () => {
    const { onStop, rerender, onStart } = setup();
    fireEvent.pointerDown(button());
    rerender(
      <PushToTalk
        unavailable
        busy
        micOpen={false}
        onStart={onStart}
        onStop={onStop}
      />,
    );
    expect(onStop).toHaveBeenCalledTimes(1);
    expect(button()).toBeDisabled();
    rerender(
      <PushToTalk
        unavailable={false}
        busy
        micOpen={false}
        onStart={onStart}
        onStop={onStop}
      />,
    );
    expect(button()).toBeDisabled();
  });

  it("stays enabled while held even though the capture made the request busy", () => {
    const { rerender, onStart, onStop } = setup();
    fireEvent.pointerDown(button());
    rerender(
      <PushToTalk
        unavailable={false}
        busy
        micOpen={false}
        onStart={onStart}
        onStop={onStop}
      />,
    );
    expect(button()).toBeEnabled();
  });

  it("says the microphone is starting until it is open, then to speak", () => {
    const { rerender, onStart, onStop } = setup();
    expect(button()).toHaveTextContent("Hold to talk");
    fireEvent.pointerDown(button());
    expect(button()).toHaveTextContent("Starting microphone…");
    rerender(
      <PushToTalk
        unavailable={false}
        busy
        micOpen
        onStart={onStart}
        onStop={onStop}
      />,
    );
    expect(button()).toHaveTextContent("Speak now");
  });
});
