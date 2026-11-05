import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PushToTalk, TAP_MS } from "./PushToTalk";

type Props = React.ComponentProps<typeof PushToTalk>;

function setup(props: Partial<Props> = {}) {
  const onStart = vi.fn(() => Promise.resolve());
  const onStop = vi.fn();
  const base: Props = {
    unavailable: false,
    busy: false,
    listening: false,
    micOpen: false,
    onStart,
    onStop,
  };
  const view = render(<PushToTalk {...base} {...props} />);
  const again = (p: Partial<Props>) =>
    view.rerender(<PushToTalk {...base} {...props} {...p} />);
  return { onStart, onStop, again, ...view };
}

const button = () =>
  screen.getByRole("button", { name: /talk|starting microphone|speak now/i });
const press = () => fireEvent.pointerDown(button());
const release = () => fireEvent.pointerUp(button());
const wait = (ms: number) =>
  act(() => {
    vi.advanceTimersByTime(ms);
  });
/** A quick press and release. */
const tap = () => {
  press();
  wait(50);
  release();
};

describe("PushToTalk", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("a hold starts on press and sends once on release", () => {
    const { onStart, onStop } = setup();
    press();
    expect(onStart).toHaveBeenCalledTimes(1);
    expect(button()).toHaveAttribute("aria-pressed", "true");
    wait(TAP_MS + 100);
    release();
    release();
    expect(onStop).toHaveBeenCalledTimes(1);
    expect(button()).toHaveAttribute("aria-pressed", "false");
  });

  it("a tap does not send on release; the capture keeps running", () => {
    const { onStart, onStop } = setup();
    tap();
    expect(onStart).toHaveBeenCalledTimes(1);
    expect(onStop).not.toHaveBeenCalled();
    expect(button()).toHaveAttribute("aria-pressed", "true");
  });

  it("a second press during a tap sends what was said so far, once", () => {
    const { onStart, onStop } = setup();
    tap();
    press();
    release();
    expect(onStop).toHaveBeenCalledTimes(1);
    expect(onStart).toHaveBeenCalledTimes(1);
    expect(button()).toHaveAttribute("aria-pressed", "false");
  });

  it("a tap-started capture that ends by itself puts the button back", () => {
    const { onStop, again } = setup();
    tap();
    again({ listening: true, micOpen: true });
    expect(button()).toHaveAttribute("aria-pressed", "true");
    again({ listening: false, micOpen: false });
    expect(button()).toHaveAttribute("aria-pressed", "false");
    expect(onStop).not.toHaveBeenCalled();
  });

  it("does not leave the tap state if listening has not been seen yet", () => {
    const { again } = setup();
    tap();
    again({ listening: false });
    expect(button()).toHaveAttribute("aria-pressed", "true");
  });

  it("lets go of a tap the backend never ended", () => {
    setup();
    tap();
    wait(26_000);
    expect(button()).toHaveAttribute("aria-pressed", "false");
  });

  it("works with the space bar and ignores key repeat", () => {
    const { onStart, onStop } = setup();
    fireEvent.keyDown(button(), { key: " " });
    fireEvent.keyDown(button(), { key: " ", repeat: true });
    expect(onStart).toHaveBeenCalledTimes(1);
    wait(TAP_MS + 100);
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
  ])("stops a hold on %s", (_name, ending) => {
    const { onStop } = setup();
    press();
    act(() => {
      ending();
    });
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("focus leaving does not end a tap, but hiding the window does", () => {
    const { onStop } = setup();
    tap();
    fireEvent.blur(button());
    expect(onStop).not.toHaveBeenCalled();
    Object.defineProperty(document, "hidden", {
      configurable: true,
      get: () => true,
    });
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    Object.defineProperty(document, "hidden", {
      configurable: true,
      get: () => false,
    });
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("goes back to idle when the start is refused", async () => {
    vi.useRealTimers();
    const onStart = vi.fn(() => Promise.reject(new Error("refused")));
    const { onStop } = setup({ onStart });
    await act(async () => {
      press();
    });
    expect(button()).toHaveAttribute("aria-pressed", "false");
    release();
    expect(onStop).not.toHaveBeenCalled();
  });

  it("is disabled while muted or while another request runs, and muting ends a capture", () => {
    const { onStop, again } = setup();
    tap();
    again({ unavailable: true, busy: true });
    expect(onStop).toHaveBeenCalledTimes(1);
    expect(button()).toBeDisabled();
    again({ unavailable: false, busy: true });
    expect(button()).toBeDisabled();
  });

  it("stays enabled while capturing even though the capture made the request busy", () => {
    const { again } = setup();
    press();
    again({ busy: true, listening: true });
    expect(button()).toBeEnabled();
  });

  it("says the microphone is starting until it is open, then how to finish", () => {
    const { again } = setup();
    expect(button()).toHaveTextContent("Hold or tap to talk");
    press();
    expect(button()).toHaveTextContent("Starting microphone…");
    again({ busy: true, listening: true, micOpen: true });
    expect(button()).toHaveTextContent("release to send");
    wait(50);
    release(); // a tap
    expect(button()).toHaveTextContent("press to send");
  });
});
