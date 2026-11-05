import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MockTransport } from "../transport/mock/MockTransport";
import { SettingsPanel } from "./SettingsPanel";

async function open(t = new MockTransport()) {
  render(<SettingsPanel transport={t} />);
  fireEvent.click(await screen.findByRole("tab", { name: "Models" }));
  await screen.findByTestId("mic-in-use");
  return t;
}
const select = () => screen.getByLabelText("Microphone to listen to");

describe("Settings → Models → Microphone", () => {
  it("shows which microphone is really in use and warns about a wireless headset", async () => {
    await open();
    expect(screen.getByTestId("mic-in-use")).toHaveTextContent(
      "In use: Wireless headset (MOCK)",
    );
    expect(screen.getByTestId("mic-wireless")).toBeInTheDocument();
  });

  it("choosing the computer's own microphone saves it and drops the warning", async () => {
    const t = await open();
    await act(async () => {
      fireEvent.change(select(), {
        target: { value: "MacBook Air Microphone (MOCK)" },
      });
    });
    expect(screen.getByTestId("mic-in-use")).toHaveTextContent(
      "In use: MacBook Air Microphone (MOCK)",
    );
    expect(screen.queryByTestId("mic-wireless")).not.toBeInTheDocument();
    expect((await t.voiceStatus()).inputDevice).toBe(
      "MacBook Air Microphone (MOCK)",
    );
  });

  it("goes back to the system default", async () => {
    const t = await open();
    await act(async () => {
      fireEvent.change(select(), {
        target: { value: "MacBook Air Microphone (MOCK)" },
      });
    });
    await act(async () => {
      fireEvent.change(select(), { target: { value: "" } });
    });
    expect((await t.voiceStatus()).inputDevice).toBeNull();
    expect(screen.getByTestId("mic-in-use")).toHaveTextContent(
      "Wireless headset (MOCK)",
    );
  });

  it("says when a saved microphone is not connected, and falls back to the default", async () => {
    const t = new MockTransport();
    await t.voiceSettingsSet({ inputDevice: "Gone Mic" });
    await open(t);
    expect(screen.getByTestId("mic-in-use")).toHaveTextContent(
      "the saved one is not connected",
    );
  });

  it("says when there is no microphone", async () => {
    const t = new MockTransport();
    t.voiceDevices = async () => ({
      inputs: [],
      outputs: [],
      defaultInput: null,
    });
    render(<SettingsPanel transport={t} />);
    fireEvent.click(await screen.findByRole("tab", { name: "Models" }));
    expect(await screen.findByTestId("mic-none")).toBeInTheDocument();
  });

  it("shows a refused value plainly", async () => {
    const t = await open();
    t.voiceSettingsSet = () =>
      Promise.reject({
        code: "INTERNAL",
        message: "That is not a valid microphone name",
        retryable: false,
        requestId: null,
      });
    await act(async () => {
      fireEvent.change(select(), {
        target: { value: "MacBook Air Microphone (MOCK)" },
      });
    });
    expect(screen.getByTestId("mic-error")).toHaveTextContent("not a valid");
  });
});
