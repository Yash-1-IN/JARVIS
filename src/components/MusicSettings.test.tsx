import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MockTransport } from "../transport/mock/MockTransport";
import { SettingsPanel } from "./SettingsPanel";

const ID = "0123456789abcdef0123456789abcdef";

async function open(t = new MockTransport()) {
  render(<SettingsPanel transport={t} />);
  fireEvent.click(await screen.findByRole("tab", { name: "Music" }));
  await screen.findByText("Not connected.");
  return t;
}

const field = () =>
  screen.getByLabelText("Spotify Client ID") as HTMLInputElement;
const save = () => screen.getByRole("button", { name: "Save Client ID" });
const connect = () => screen.getByRole("button", { name: "Connect Spotify" });

describe("Settings → Music (Spotify)", () => {
  it("cannot connect before a Client ID is saved, and says what a bad one is", async () => {
    await open();
    expect(connect()).toBeDisabled();
    expect(save()).toBeDisabled();

    fireEvent.change(field(), { target: { value: "not-an-id" } });
    await act(async () => {
      fireEvent.click(save());
    });
    expect(screen.getByTestId("spotify-error")).toHaveTextContent(
      "does not look like a Spotify Client ID",
    );
    expect(field().value).toBe("not-an-id"); // kept for correction
    expect(connect()).toBeDisabled();
  });

  it("saves the ID, connects, shows who is connected, and disconnects", async () => {
    const t = await open();
    fireEvent.change(field(), { target: { value: ID.toUpperCase() } });
    await act(async () => {
      fireEvent.click(save());
    });
    expect((await t.spotifyStatus()).clientId).toBe(ID);
    expect(connect()).toBeEnabled();

    await act(async () => {
      fireEvent.click(connect());
    });
    expect(screen.getByTestId("spotify-status")).toHaveTextContent(
      "Connected as Mock listener.",
    );

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    });
    expect(screen.getByTestId("spotify-status")).toHaveTextContent(
      "Not connected.",
    );
    expect((await t.spotifyStatus()).clientId).toBe(ID); // the ID is kept
  });

  it("shows a refused sign-in as it is, and Cancel is offered while waiting", async () => {
    const t = new MockTransport();
    await t.spotifySetClientId(ID);
    let release: () => void = () => {};
    t.spotifyConnect = () =>
      new Promise((_, reject) => {
        release = () =>
          reject({
            code: "INTERNAL",
            message: "Spotify sign-in was cancelled.",
            retryable: false,
            requestId: null,
          });
      });
    await open(t);
    expect(field().value).toBe(ID);
    await act(async () => {
      fireEvent.click(connect());
    });
    expect(screen.getByRole("status")).toHaveTextContent(
      "Waiting for you to finish in the browser",
    );
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
      release();
    });
    expect(screen.getByTestId("spotify-error")).toHaveTextContent(
      "Spotify sign-in was cancelled.",
    );
    expect(connect()).toBeEnabled();
  });

  it("never asks for or shows a secret, and explains the setup", async () => {
    await open();
    expect(screen.queryByLabelText(/secret|token/i)).toBeNull();
    expect(
      screen.getByText(/http:\/\/127\.0\.0\.1:43597\/callback/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/nothing is sent to Anthropic/i),
    ).toBeInTheDocument();
  });
});
