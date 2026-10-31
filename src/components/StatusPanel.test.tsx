import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MIC_LABELS, PHASE_LABELS } from "../state/labels";
import type { AssistantPhase } from "../types/generated/AssistantPhase";
import type { MicState } from "../types/generated/MicState";
import type { Snapshot } from "../types/generated/Snapshot";
import { StatusPanel } from "./StatusPanel";

const base: Snapshot = {
  seq: 1,
  mic: "off",
  phase: "idle",
  activeRequestId: null,
  network: "online",
  cloudInUse: false,
  pendingApprovals: [],
  lastError: null,
};

describe("StatusPanel", () => {
  it("renders a distinct text label for every mic × phase combination", () => {
    const seen = new Set<string>();
    for (const mic of Object.keys(MIC_LABELS) as MicState[]) {
      for (const phase of Object.keys(PHASE_LABELS) as AssistantPhase[]) {
        const { container, unmount } = render(
          <StatusPanel snapshot={{ ...base, mic, phase }} />,
        );
        const text = container.querySelector("[aria-live]")!.textContent!;
        expect(text).toContain(MIC_LABELS[mic]);
        expect(text).toContain(PHASE_LABELS[phase]);
        seen.add(text);
        unmount();
      }
    }
    expect(seen.size).toBe(
      Object.keys(MIC_LABELS).length * Object.keys(PHASE_LABELS).length,
    );
  });

  it("announces state politely and shows errors as alerts", () => {
    render(
      <StatusPanel
        snapshot={{
          ...base,
          phase: "error",
          lastError: {
            code: "INTERNAL",
            message: "Boom",
            retryable: false,
            requestId: null,
          },
        }}
      />,
    );
    expect(
      screen.getByText(PHASE_LABELS.error).closest("[aria-live]"),
    ).toHaveAttribute("aria-live", "polite");
    expect(screen.getByRole("alert")).toHaveTextContent("Boom");
  });

  it("flags when data is leaving the machine", () => {
    render(<StatusPanel snapshot={{ ...base, cloudInUse: true }} />);
    expect(screen.getByText(/Sending data to the cloud/)).toBeInTheDocument();
  });
});
