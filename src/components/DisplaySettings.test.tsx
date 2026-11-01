import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("./hologram/renderer", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./hologram/renderer")>()),
  hasWebGL: () => false,
}));

import { DisplaySettings } from "./DisplaySettings";
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
});
