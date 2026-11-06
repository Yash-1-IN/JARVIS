import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("./hologram/renderer", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./hologram/renderer")>()),
  hasWebGL: () => false,
}));

import { DisplaySettings } from "./DisplaySettings";
import { usePanelSplit } from "../state/layoutPref";
import { useHologramPref } from "./hologram/renderer";

describe("DisplaySettings", () => {
  it("chooses the hologram and says when 3D is unavailable", () => {
    useHologramPref.setState({ pref: "auto" });
    render(<DisplaySettings />);
    expect(screen.getByLabelText("Automatic")).toBeChecked();
    fireEvent.click(screen.getByLabelText("2D orb"));
    expect(useHologramPref.getState().pref).toBe("2d");
    expect(localStorage.getItem("jarvis.hologram")).toBe("2d");
    expect(screen.getByTestId("no-webgl")).toBeInTheDocument();
    useHologramPref.getState().setPref("auto");
  });

  it("chooses where panels open and remembers it on this computer", () => {
    usePanelSplit.setState({ split: "rail" });
    render(<DisplaySettings />);
    expect(screen.getByLabelText("Over the hologram side")).toBeChecked();
    fireEvent.click(screen.getByLabelText("A pop-up window"));
    expect(usePanelSplit.getState().split).toBe("popup");
    expect(localStorage.getItem("jarvis.panelSplit")).toBe("popup");
    fireEvent.click(screen.getByLabelText("Over the conversation"));
    expect(usePanelSplit.getState().split).toBe("stage");
    expect(localStorage.getItem("jarvis.panelSplit")).toBe("stage");
    fireEvent.click(screen.getByLabelText("Across the whole window"));
    expect(usePanelSplit.getState().split).toBe("both");
    usePanelSplit.getState().setSplit("rail");
  });
});
