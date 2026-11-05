import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MockTransport } from "../transport/mock/MockTransport";
import { SettingsPanel } from "./SettingsPanel";

async function open(t = new MockTransport()) {
  render(<SettingsPanel transport={t} />);
  fireEvent.click(await screen.findByRole("tab", { name: "Models" }));
  await screen.findByText("The speech models are not installed yet.");
  return t;
}

const download = () =>
  screen.getByRole("button", { name: /download speech models|downloading/i });

describe("Settings → Models", () => {
  afterEach(() => vi.useRealTimers());

  it("downloads with visible progress and ends installed, with no button left", async () => {
    const t = await open();
    vi.useFakeTimers();
    await act(async () => {
      fireEvent.click(download());
    });
    expect(download()).toBeDisabled();
    expect(screen.getByTestId("models-progress")).toHaveTextContent(
      "Starting…",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(screen.getByTestId("models-progress")).toHaveTextContent(
      "100 of 251 MB (39%)",
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(screen.getByTestId("models-status")).toHaveTextContent(
      "The speech models are installed.",
    );
    expect(
      screen.queryByRole("button", { name: /download/i }),
    ).not.toBeInTheDocument();
    expect((await t.voiceStatus()).modelsReady).toBe(true);
  });

  it("shows a failure plainly and lets the person try again", async () => {
    const t = new MockTransport();
    const real = t.voiceModelsInstall.bind(t);
    t.voiceModelsInstall = vi
      .fn()
      .mockRejectedValueOnce({
        code: "NETWORK_OFFLINE",
        message: "The speech models could not be downloaded.",
        retryable: true,
        requestId: null,
      })
      .mockImplementation(real);
    await open(t);
    await act(async () => {
      fireEvent.click(download());
    });
    expect(screen.getByTestId("models-error")).toHaveTextContent(
      "could not be downloaded",
    );
    expect(download()).toBeEnabled();
  });

  it("says when the status cannot be read", async () => {
    const t = new MockTransport();
    t.voiceStatus = () =>
      Promise.reject({
        code: "INTERNAL",
        message: "nope",
        retryable: false,
        requestId: null,
      });
    render(<SettingsPanel transport={t} />);
    fireEvent.click(await screen.findByRole("tab", { name: "Models" }));
    expect(await screen.findByTestId("models-error")).toBeInTheDocument();
  });
});
