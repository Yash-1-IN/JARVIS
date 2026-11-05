import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MockTransport } from "../transport/mock/MockTransport";
import { SettingsPanel } from "./SettingsPanel";
import { WAKE_NOTICE } from "./WakeWordSettings";

async function open(t = new MockTransport()) {
  render(<SettingsPanel transport={t} />);
  fireEvent.click(await screen.findByRole("tab", { name: "Models" }));
  await screen.findByTestId("wake-status");
  return t;
}
const btn = (name: string | RegExp) => screen.getByRole("button", { name });

describe("Settings → Models → Wake word", () => {
  it("is off by default and shows the full licence notice before anything is turned on", async () => {
    const t = await open();
    expect(screen.getByTestId("wake-status")).toHaveTextContent("Off.");
    expect((await t.voiceStatus()).wakeEnabled).toBe(false);
    fireEvent.click(btn("Turn on the wake word"));
    // The notice is shown, word for word, and nothing has been saved yet.
    expect(screen.getByTestId("wake-notice")).toHaveTextContent(WAKE_NOTICE);
    expect(WAKE_NOTICE).toContain("CC BY-NC-SA 4.0");
    expect(WAKE_NOTICE).toContain("never saved");
    const s = await t.voiceStatus();
    expect(s.wakeEnabled).toBe(false);
    expect(s.wakeNoticeAccepted).toBe(false);
  });

  it("Cancel leaves it off and unaccepted", async () => {
    const t = await open();
    fireEvent.click(btn("Turn on the wake word"));
    fireEvent.click(btn("Cancel"));
    expect(screen.queryByTestId("wake-notice")).not.toBeInTheDocument();
    expect(await t.voiceStatus()).toMatchObject({
      wakeEnabled: false,
      wakeNoticeAccepted: false,
    });
  });

  it("accepting sends the notice and the switch together, downloads the files, and says it is listening", async () => {
    const t = await open();
    fireEvent.click(btn("Turn on the wake word"));
    await act(async () => {
      fireEvent.click(btn("Accept and turn on"));
      await new Promise((r) => setTimeout(r, 1300));
    });
    expect(await t.voiceStatus()).toMatchObject({
      wakeEnabled: true,
      wakeNoticeAccepted: true,
      wakeReady: true,
    });
    expect(screen.getByTestId("wake-status")).toHaveTextContent(
      "listening for",
    );
    // The microphone shows the wake-word waiting state.
    expect((await t.getSnapshot()).mic).toBe("standby");
  });

  it("turns off, and turning on again does not ask for the notice a second time", async () => {
    const t = new MockTransport();
    await t.voiceSettingsSet({ wakeNoticeAccepted: true, wakeEnabled: true });
    await open(t);
    await act(async () => {
      fireEvent.click(btn("Turn off the wake word"));
    });
    expect(await t.voiceStatus()).toMatchObject({
      wakeEnabled: false,
      wakeNoticeAccepted: true,
    });
    fireEvent.click(btn("Turn on the wake word"));
    expect(screen.queryByTestId("wake-notice")).not.toBeInTheDocument();
  });

  it("the gate holds below the window too: turning it on without the notice is refused", async () => {
    const t = new MockTransport();
    await expect(
      t.voiceSettingsSet({ wakeEnabled: true }),
    ).rejects.toMatchObject({
      message: "Read and accept the wake word notice first.",
    });
    expect((await t.voiceStatus()).wakeEnabled).toBe(false);
  });

  it("says plainly where the wake word is not offered, with no switch", async () => {
    const t = new MockTransport();
    const real = t.voiceStatus.bind(t);
    t.voiceStatus = async () => ({ ...(await real()), wakeSupported: false });
    render(<SettingsPanel transport={t} />);
    fireEvent.click(await screen.findByRole("tab", { name: "Models" }));
    expect(await screen.findByTestId("wake-unsupported")).toHaveTextContent(
      "not available on this computer",
    );
    expect(
      screen.queryByRole("button", { name: "Turn on the wake word" }),
    ).not.toBeInTheDocument();
  });

  it("shows a failure plainly", async () => {
    const t = await open();
    t.voiceSettingsSet = () =>
      Promise.reject({
        code: "INTERNAL",
        message: "The wake word is not available on this computer.",
        retryable: false,
        requestId: null,
      });
    fireEvent.click(btn("Turn on the wake word"));
    await act(async () => {
      fireEvent.click(btn("Accept and turn on"));
    });
    expect(screen.getByTestId("wake-error")).toHaveTextContent("not available");
  });
});
