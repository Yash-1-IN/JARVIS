import { describe, expect, it } from "vitest";
import { MockTransport } from "./MockTransport";

/**
 * The release-installer workflow (`.github/workflows/installers.yml`, issue #86) searches the
 * built frontend for these exact strings to prove the mock transport was not bundled into an
 * installer. If the mock stops containing them, that guard would pass forever without checking
 * anything. Change them only together with the workflow. Two are searched so that renaming one
 * cannot switch the guard off.
 */
const GUARD_ID = "mock.app.music";
const GUARD_NAME = "Mock Music";

describe("release guard literals", () => {
  it("are still present in the mock transport", async () => {
    const apps = await new MockTransport().appsInstalled();
    expect(apps.map((a) => a.id)).toContain(GUARD_ID);
    expect(apps.map((a) => a.name)).toContain(GUARD_NAME);
  });
});
