import { beforeEach, describe, expect, it } from "vitest";
import { useGlassPref, usePanelSplit } from "./layoutPref";

describe("layoutPref", () => {
  beforeEach(() => {
    localStorage.clear();
    usePanelSplit.setState({ split: "popup" });
    useGlassPref.setState({ glass: "auto" });
  });

  it("persists the panel split choice in localStorage", () => {
    expect(usePanelSplit.getState().split).toBe("popup");
    usePanelSplit.getState().setSplit("stage");
    expect(usePanelSplit.getState().split).toBe("stage");
    expect(localStorage.getItem("jarvis.panelSplit")).toBe("stage");
  });

  it("persists the glass effect choice in localStorage", () => {
    expect(useGlassPref.getState().glass).toBe("auto");
    useGlassPref.getState().setGlass("on");
    expect(useGlassPref.getState().glass).toBe("on");
    expect(localStorage.getItem("jarvis.glassEffect")).toBe("on");
    useGlassPref.getState().setGlass("off");
    expect(useGlassPref.getState().glass).toBe("off");
    expect(localStorage.getItem("jarvis.glassEffect")).toBe("off");
  });
});
