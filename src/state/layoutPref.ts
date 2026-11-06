import { create } from "zustand";

/**
 * Where an open panel (Settings, Activity, Memory, History) appears. Four layouts are offered so
 * they can be compared (issue #144):
 *
 * - `popup`: a **pop-up window** over the app, with a dimmed background, like the settings of a
 *   desktop app: sections down the left, the content beside them, closed with Esc, the Close button
 *   or a click outside. Nothing on the page moves. This is the default.
 * - `rail`: the panel splits the **talk column** (hologram side): the panel above, the hologram, status and
 *   talk button below it. The conversation is not touched.
 * - `stage`: the panel splits the **conversation** side: the panel above the conversation. The talk
 *   column is not touched.
 * - `both`: the panel runs across the **whole width**, above the talk column and the conversation.
 *
 * A look-and-feel choice only: stored on this computer, never sent to the backend.
 */
export type PanelSplit = "popup" | "rail" | "stage" | "both";

const KEY = "jarvis.panelSplit";
const VALUES: readonly PanelSplit[] = ["popup", "rail", "stage", "both"];

function read(): PanelSplit {
  try {
    const v = localStorage.getItem(KEY);
    if (v && (VALUES as readonly string[]).includes(v)) return v as PanelSplit;
  } catch {
    // Storage can be unavailable; the default applies.
  }
  return "popup";
}

export const usePanelSplit = create<{
  split: PanelSplit;
  setSplit: (s: PanelSplit) => void;
}>((set) => ({
  split: read(),
  setSplit: (split) => {
    try {
      localStorage.setItem(KEY, split);
    } catch {
      // The choice still applies until restart.
    }
    set({ split });
  },
}));
