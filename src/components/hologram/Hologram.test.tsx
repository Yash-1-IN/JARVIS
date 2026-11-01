import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const env = { webgl: true, linux: false, throw3d: false };

vi.mock("./renderer", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./renderer")>()),
  hasWebGL: () => env.webgl,
  isLinux: () => env.linux,
}));

// jsdom has no WebGL: a stand-in records what the 3D view was asked to do.
vi.mock("./Hologram3D", () => ({
  default: (p: {
    frameloop: string;
    still: boolean;
    motion: string;
    onLost: () => void;
  }) => {
    if (env.throw3d) throw new Error("no WebGL context");
    return (
      <div
        data-testid="hologram-3d"
        data-frameloop={p.frameloop}
        data-still={String(p.still)}
        data-motion={p.motion}
      >
        <button onClick={p.onLost}>lose context</button>
      </div>
    );
  },
}));

import { Hologram, resetWebglCacheForTests } from "./Hologram";
import { useHologramPref } from "./renderer";

function setHidden(hidden: boolean) {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => (hidden ? "hidden" : "visible"),
  });
  document.dispatchEvent(new Event("visibilitychange"));
}

function setReducedMotion(on: boolean) {
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: on && q.includes("reduce"),
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

describe("Hologram", () => {
  beforeEach(() => {
    Object.assign(env, { webgl: true, linux: false, throw3d: false });
    resetWebglCacheForTests();
    useHologramPref.setState({ pref: "auto", choice: 0 });
    setHidden(false);
    setReducedMotion(false);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("shows 3D where it is safe, rendering on demand while idle", async () => {
    render(<Hologram phase="idle" mic="off" />);
    const h = await screen.findByTestId("hologram-3d");
    expect(h).toHaveAttribute("data-frameloop", "demand");
  });

  it("animates while listening and stops entirely while hidden", async () => {
    const { rerender } = render(<Hologram phase="listening" mic="capturing" />);
    expect(await screen.findByTestId("hologram-3d")).toHaveAttribute(
      "data-frameloop",
      "always",
    );
    act(() => setHidden(true));
    rerender(<Hologram phase="listening" mic="capturing" />);
    expect(screen.getByTestId("hologram-3d")).toHaveAttribute(
      "data-frameloop",
      "never",
    );
  });

  it("with reduced motion it stays still and renders on demand", async () => {
    setReducedMotion(true);
    render(<Hologram phase="speaking" mic="capturing" />);
    const h = await screen.findByTestId("hologram-3d");
    expect(h).toHaveAttribute("data-frameloop", "demand");
    expect(h).toHaveAttribute("data-still", "true");
  });

  it("uses the 2D orb without WebGL, on Linux by default, or when chosen", async () => {
    env.webgl = false;
    const a = render(<Hologram phase="idle" mic="off" />);
    expect(screen.getByTestId("orb")).toBeInTheDocument();
    expect(screen.queryByTestId("hologram-3d")).toBeNull();
    a.unmount();

    resetWebglCacheForTests();
    Object.assign(env, { webgl: true, linux: true });
    const b = render(<Hologram phase="idle" mic="off" />);
    expect(screen.getByTestId("orb")).toBeInTheDocument();
    b.unmount();

    // On Linux the user can still ask for 3D.
    useHologramPref.setState({ pref: "3d" });
    const c = render(<Hologram phase="idle" mic="off" />);
    expect(await screen.findByTestId("hologram-3d")).toBeInTheDocument();
    c.unmount();

    env.linux = false;
    useHologramPref.setState({ pref: "2d" });
    render(<Hologram phase="idle" mic="off" />);
    expect(screen.getByTestId("orb")).toBeInTheDocument();
  });

  it("falls back to 2D when the 3D view fails or loses its context", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    env.throw3d = true;
    const a = render(<Hologram phase="idle" mic="off" />);
    // Let the lazy 3D module load and fail, then check the boundary's 2D orb.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    expect(screen.queryByTestId("hologram-3d")).toBeNull();
    expect(screen.getByTestId("orb")).toBeInTheDocument();
    a.unmount();

    env.throw3d = false;
    render(<Hologram phase="idle" mic="off" />);
    await act(async () =>
      fireEvent.click(await screen.findByText("lose context")),
    );
    expect(screen.queryByTestId("hologram-3d")).toBeNull();
    expect(screen.getByTestId("orb")).toBeInTheDocument();
  });

  it("switches back to 3D after 2D was chosen (regression)", async () => {
    render(<Hologram phase="idle" mic="off" />);
    expect(await screen.findByTestId("hologram-3d")).toBeInTheDocument();
    act(() => useHologramPref.getState().setPref("2d"));
    expect(screen.queryByTestId("hologram-3d")).toBeNull();
    act(() => useHologramPref.getState().setPref("3d"));
    expect(await screen.findByTestId("hologram-3d")).toBeInTheDocument();
  });

  it("after a real 3D failure, choosing again gives 3D a fresh try", async () => {
    render(<Hologram phase="idle" mic="off" />);
    await act(async () =>
      fireEvent.click(await screen.findByText("lose context")),
    );
    expect(screen.queryByTestId("hologram-3d")).toBeNull();
    act(() => useHologramPref.getState().setPref("3d"));
    expect(await screen.findByTestId("hologram-3d")).toBeInTheDocument();
  });
});
