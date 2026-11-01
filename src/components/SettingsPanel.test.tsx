import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MockTransport } from "../transport/mock/MockTransport";
import { SettingsPanel } from "./SettingsPanel";

async function open(t = new MockTransport()) {
  render(<SettingsPanel transport={t} />);
  await screen.findByTestId("key-status");
  return t;
}

describe("SettingsPanel", () => {
  it("saves a key, clears the field, and shows only the hint", async () => {
    const t = await open();
    const spy = vi.spyOn(t, "setApiKey");
    const input = screen.getByLabelText("API key") as HTMLInputElement;
    expect(input.type).toBe("password");

    fireEvent.change(input, {
      target: { value: "sk-ant-mock-0123456789abWXYZ" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save key" }));
    });
    expect(spy).toHaveBeenCalledWith("sk-ant-mock-0123456789abWXYZ");
    expect(
      (screen.getByLabelText("Replace key") as HTMLInputElement).value,
    ).toBe("");
    expect(screen.getByTestId("key-status")).toHaveTextContent("ending …WXYZ");
    expect(document.body.textContent).not.toContain("sk-ant-mock");
    expect(screen.getByRole("status")).toHaveTextContent("API key saved.");
  });

  it("shows a rejected key as an error and stores nothing", async () => {
    await open();
    fireEvent.change(screen.getByLabelText("API key"), {
      target: { value: "bad-0123456789abcdef" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save key" }));
    });
    expect(screen.getByRole("alert")).toHaveTextContent("API key problem");
    expect(screen.getByTestId("key-status")).toHaveTextContent(
      "No API key set.",
    );
    expect((screen.getByLabelText("API key") as HTMLInputElement).value).toBe(
      "",
    );
  });

  it("warns when the key is session-only", async () => {
    const t = new MockTransport();
    await t.setApiKey("sk-ant-mock-0123456789abWXYZ");
    t.simulateSessionOnlyKey();
    await open(t);
    expect(screen.getByRole("alert")).toHaveTextContent(
      "kept only until JARVIS quits",
    );
  });

  it("sends only changed caps, in micro-dollars, and null for a cleared cap", async () => {
    const t = new MockTransport();
    await t.updateSettings({ monthlyCapMicroUsd: 20_000_000 });
    await open(t);
    const spy = vi.spyOn(t, "updateSettings");

    fireEvent.change(screen.getByLabelText("Daily cap (USD)"), {
      target: { value: "2.50" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save caps" }));
    });
    expect(spy).toHaveBeenLastCalledWith({ dailyCapMicroUsd: 2_500_000 });

    fireEvent.change(screen.getByLabelText("Monthly cap (USD)"), {
      target: { value: "" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save caps" }));
    });
    expect(spy).toHaveBeenLastCalledWith({ monthlyCapMicroUsd: null });
    expect(screen.getByTestId("usage")).toHaveTextContent("(no cap)");
  });

  it("rejects malformed caps without calling the backend", async () => {
    const t = await open();
    const spy = vi.spyOn(t, "updateSettings");
    fireEvent.change(screen.getByLabelText("Daily cap (USD)"), {
      target: { value: "-3" },
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Save caps" }));
    });
    expect(spy).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("dollar amounts");
  });

  it("changes the model from the priced list", async () => {
    const t = await open();
    const spy = vi.spyOn(t, "updateSettings");
    await act(async () => {
      fireEvent.change(screen.getByLabelText("Model"), {
        target: { value: "mock-model-smart" },
      });
    });
    expect(spy).toHaveBeenCalledWith({ model: "mock-model-smart" });
    expect((screen.getByLabelText("Model") as HTMLSelectElement).value).toBe(
      "mock-model-smart",
    );
  });

  it("updates usage live from usage.updated", async () => {
    vi.useFakeTimers();
    const t = new MockTransport();
    render(<SettingsPanel transport={t} />);
    await act(async () => {});
    expect(screen.getByTestId("usage")).toHaveTextContent("$0.00 (no cap)");
    await act(async () => {
      await t.submitText("hi", () => {});
      vi.advanceTimersByTime(60_000);
    });
    expect(screen.getByTestId("usage")).toHaveTextContent("$0.0015 (no cap)");
    vi.useRealTimers();
  });
});
