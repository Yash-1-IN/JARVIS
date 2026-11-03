import { describe, expect, it } from "vitest";
import { MockTransport } from "./MockTransport";

/**
 * The release-installer workflow (`.github/workflows/installers.yml`, issue #86) searches the
 * built frontend for this exact string to prove the mock transport was not bundled into an
 * installer. If the mock stops containing it, that guard would pass forever without checking
 * anything. Change it only together with the workflow.
 */
const GUARD_LITERAL = "mock.app.music";

describe("release guard literal", () => {
  it("is still present in the mock transport", async () => {
    const apps = await new MockTransport().appsInstalled();
    expect(apps.map((a) => a.id)).toContain(GUARD_LITERAL);
  });
});
