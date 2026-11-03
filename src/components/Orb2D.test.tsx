import { act, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MIC_LABELS, PHASE_LABELS } from "../state/labels";
import type { AssistantPhase } from "../types/generated/AssistantPhase";
import type { MicState } from "../types/generated/MicState";
import { Orb2D, orbMotion } from "./Orb2D";

const PHASES = Object.keys(PHASE_LABELS) as AssistantPhase[];
const MICS = Object.keys(MIC_LABELS) as MicState[];

describe("Orb2D", () => {
  it("is static when idle, so an idle window renders no animation frames", () => {
    for (const mic of MICS.filter((m) => m !== "capturing")) {
      expect(orbMotion("idle", mic), mic).toBe("none");
    }
  });

  it("animates every active phase", () => {
    for (const p of PHASES.filter((p) => p !== "idle" && p !== "error")) {
      expect(orbMotion(p, "standby"), p).not.toBe("none");
    }
  });

  it("is decorative: hidden from screen readers (StatusPanel has the text)", () => {
    render(<Orb2D phase="thinking" mic="standby" />);
    expect(screen.getByTestId("orb")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByTestId("orb")).toHaveAttribute("data-motion", "spin");
  });

  it("pauses while the window is hidden", () => {
    render(<Orb2D phase="speaking" mic="standby" />);
    expect(screen.getByTestId("orb")).not.toHaveClass("orb-paused");
    Object.defineProperty(document, "visibilityState", {
      value: "hidden",
      configurable: true,
    });
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(screen.getByTestId("orb")).toHaveClass("orb-paused");
    Object.defineProperty(document, "visibilityState", {
      value: "visible",
      configurable: true,
    });
  });

  it("marks error and mute by shape, not colour alone", () => {
    const { container, rerender } = render(
      <Orb2D phase="error" mic="standby" />,
    );
    expect(container.querySelector(".orb-mark")).not.toBeNull();
    rerender(<Orb2D phase="idle" mic="muted" />);
    expect(container.querySelector(".orb-mark")).not.toBeNull();
  });

  it("flares once for each change of state, and not when it first appears", () => {
    const { container, rerender } = render(
      <Orb2D phase="idle" mic="standby" />,
    );
    expect(container.querySelector(".orb-flare")).toBeNull();
    rerender(<Orb2D phase="thinking" mic="standby" />);
    const first = container.querySelector(".orb-flare");
    expect(first).not.toBeNull();
    // The same state again is not a change.
    rerender(<Orb2D phase="thinking" mic="standby" />);
    expect(container.querySelector(".orb-flare")).toBe(first);
    // A new change is a new element, which restarts the animation.
    rerender(<Orb2D phase="idle" mic="standby" />);
    const second = container.querySelector(".orb-flare");
    expect(second).not.toBeNull();
    expect(second).not.toBe(first);
    // A mic change flares too.
    rerender(<Orb2D phase="idle" mic="muted" />);
    expect(container.querySelector(".orb-flare")).not.toBe(second);
    // The flare never adds text or changes what screen readers get.
    expect(container.querySelector(".orb")?.getAttribute("aria-hidden")).toBe(
      "true",
    );
    expect(container.textContent).toBe("");
  });
});
