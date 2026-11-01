import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MockTransport } from "../transport/mock/MockTransport";
import { SettingsPanel } from "./SettingsPanel";

async function open(t = new MockTransport()) {
  render(<SettingsPanel transport={t} />);
  fireEvent.click(await screen.findByRole("tab", { name: "Permissions" }));
  await screen.findByRole("list", { name: "Installed apps" });
  return t;
}

describe("Policy settings (allowed folders, apps, auto-allow)", () => {
  it("adds and removes a folder; a refused path stays in the field", async () => {
    const t = await open();
    expect(screen.getByTestId("no-folders")).toBeInTheDocument();
    const input = screen.getByLabelText(
      "Or type a full path",
    ) as HTMLInputElement;

    fireEvent.change(input, { target: { value: "relative/path" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add folder" }));
    });
    expect(screen.getByRole("alert")).toHaveTextContent("absolute");
    expect(input.value).toBe("relative/path");

    fireEvent.change(input, { target: { value: "/home/u/Documents" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add folder" }));
    });
    expect(input.value).toBe("");
    expect((await t.getSettings()).allowedFolders).toEqual([
      "/home/u/Documents",
    ]);

    await act(async () => {
      fireEvent.click(
        screen.getByRole("button", { name: "Remove folder /home/u/Documents" }),
      );
    });
    expect((await t.getSettings()).allowedFolders).toEqual([]);
  });

  it("shows each app's id next to its name and allows by id", async () => {
    const t = await open();
    const list = screen.getByRole("list", { name: "Installed apps" });
    expect(within(list).getByText("mock.app.music")).toBeInTheDocument();
    const spy = vi.spyOn(t, "updateSettings");
    await act(async () => {
      fireEvent.click(within(list).getByLabelText(/Mock Music/));
    });
    expect(spy).toHaveBeenLastCalledWith({
      allowedApps: [{ id: "mock.app.music", name: "Mock Music" }],
    });
    expect(within(list).getByLabelText(/Mock Music/)).toBeChecked();

    await act(async () => {
      fireEvent.click(within(list).getByLabelText(/Mock Music/));
    });
    expect((await t.getSettings()).allowedApps).toEqual([]);
  });

  it("filters by name or id", async () => {
    await open();
    fireEvent.change(screen.getByLabelText("Filter apps"), {
      target: { value: "notes" },
    });
    const list = screen.getByRole("list", { name: "Installed apps" });
    expect(within(list).queryByText("Mock Music")).toBeNull();
    expect(within(list).getByText(/Mock Notes/)).toBeInTheDocument();
  });

  it("auto-allow is off by default and the hint says when it still asks", async () => {
    const t = await open();
    const box = screen.getByLabelText(/run without asking/);
    expect(box).not.toBeChecked();
    expect(
      screen.getByText(/always asks for high-risk actions/),
    ).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(box);
    });
    expect((await t.getSettings()).autoAllowMedium).toBe(true);
  });

  it("shows an allowed app that is no longer installed so it can be removed", async () => {
    const t = new MockTransport();
    await t.updateSettings({
      allowedApps: [{ id: "mock.app.notes", name: "x" }],
    });
    vi.spyOn(t, "appsInstalled").mockResolvedValue([]);
    render(<SettingsPanel transport={t} />);
    fireEvent.click(await screen.findByRole("tab", { name: "Permissions" }));
    const orphans = await screen.findByRole("list", {
      name: "Allowed apps not installed",
    });
    expect(within(orphans).getByText("mock.app.notes")).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(within(orphans).getByRole("button", { name: "Remove" }));
    });
    expect((await t.getSettings()).allowedApps).toEqual([]);
  });

  it("Choose folder adds the picked folder through the normal settings check", async () => {
    const t = await open();
    const update = vi.spyOn(t, "updateSettings");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Choose folder…" }));
    });
    expect(update).toHaveBeenLastCalledWith({
      allowedFolders: ["/Users/mock/Documents/Mock Folder"],
    });
    expect(
      screen.getByRole("list", { name: "Allowed folders" }),
    ).toHaveTextContent("/Users/mock/Documents/Mock Folder");

    // The same folder again: says so, sends nothing.
    update.mockClear();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Choose folder…" }));
    });
    expect(update).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("already allowed");
  });

  it("Cancel in the picker changes nothing", async () => {
    const t = await open();
    t.nextPickedFolder = null;
    const update = vi.spyOn(t, "updateSettings");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Choose folder…" }));
    });
    expect(update).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows why when the picker fails", async () => {
    const t = await open();
    vi.spyOn(t, "pickFolder").mockRejectedValue({
      code: "INTERNAL",
      message: "That folder's name contains characters JARVIS cannot store.",
      retryable: false,
      requestId: null,
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Choose folder…" }));
    });
    expect(screen.getByRole("alert")).toHaveTextContent("cannot store");
  });
});
