import { usePanelSplit, type PanelSplit } from "../state/layoutPref";
import {
  hasWebGL,
  isLinux,
  useHologramPref,
  type HologramPref,
} from "./hologram/renderer";

const CHOICES: { value: HologramPref; label: string; hint: string }[] = [
  {
    value: "auto",
    label: "Automatic",
    hint: "3D where it runs well; the lighter 2D orb on Linux.",
  },
  { value: "3d", label: "3D hologram", hint: "Uses the graphics card." },
  {
    value: "2d",
    label: "2D orb",
    hint: "Lightest on battery and older computers.",
  },
];

const SPLITS: { value: PanelSplit; label: string; hint: string }[] = [
  {
    value: "popup",
    label: "A pop-up window",
    hint: "Opens over the app with the page dimmed behind it; Esc or a click outside closes it.",
  },
  {
    value: "rail",
    label: "Over the hologram side",
    hint: "The panel opens above the hologram and talk button; the conversation stays whole.",
  },
  {
    value: "stage",
    label: "Over the conversation",
    hint: "The panel opens above the conversation; the hologram and talk button stay whole.",
  },
  {
    value: "both",
    label: "Across the whole window",
    hint: "The panel takes the top half of the window, above both.",
  },
];

/** Settings → Display: which hologram to draw (spec §13 M1). Stored on this computer only. */
export function DisplaySettings() {
  const { pref, setPref } = useHologramPref();
  const { split, setSplit } = usePanelSplit();
  const webgl = hasWebGL();
  return (
    <>
      <fieldset className="display-settings">
        <legend>Where panels open</legend>
        {SPLITS.map((c) => (
          <div key={c.value}>
            <label>
              <input
                type="radio"
                name="panel-split"
                value={c.value}
                checked={split === c.value}
                onChange={() => setSplit(c.value)}
              />{" "}
              {c.label}
            </label>
            <span className="hint"> {c.hint}</span>
          </div>
        ))}
      </fieldset>
      <fieldset className="display-settings">
        <legend>Hologram</legend>
        {CHOICES.map((c) => (
          <div key={c.value}>
            <label>
              <input
                type="radio"
                name="hologram"
                value={c.value}
                checked={pref === c.value}
                onChange={() => setPref(c.value)}
              />{" "}
              {c.label}
            </label>
            <span className="hint"> {c.hint}</span>
          </div>
        ))}
        {!webgl && (
          <p className="hint" data-testid="no-webgl">
            3D is not available on this computer, so the 2D orb is shown.
          </p>
        )}
        {webgl && isLinux() && pref === "auto" && (
          <p className="hint">On Linux, Automatic shows the 2D orb.</p>
        )}
        <p className="hint">
          With &quot;reduce motion&quot; turned on in your system settings, the
          hologram stays still.
        </p>
      </fieldset>
    </>
  );
}
