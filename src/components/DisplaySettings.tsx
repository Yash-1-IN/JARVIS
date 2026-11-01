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

/** Settings → Display: which hologram to draw (spec §13 M1). Stored on this computer only. */
export function DisplaySettings() {
  const { pref, setPref } = useHologramPref();
  const webgl = hasWebGL();
  return (
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
  );
}
