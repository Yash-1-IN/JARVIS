import { describe, expect, it } from "vitest";
import { MockMemoryStore, rememberText } from "./memory";

describe("MockMemoryStore (same rules as the backend store)", () => {
  it("proposes, announces, de-duplicates and accepts", () => {
    const s = new MockMemoryStore();
    const seen: string[] = [];
    s.onProposed((m) => seen.push(m.content));
    const m = s.propose("  Likes\ttea \n");
    expect(m.content).toBe("Likes tea");
    expect(m.status).toBe("proposed");
    expect(s.propose("Likes tea").id).toBe(m.id);
    expect(seen).toEqual(["Likes tea", "Likes tea"]);
    expect(s.accept(m.id).status).toBe("active");
    // An accepted text is not proposed again.
    expect(s.propose("Likes tea").status).toBe("active");
    expect(seen).toHaveLength(2);
  });

  it("counts the limit in code points and rejects empty text", () => {
    const s = new MockMemoryStore();
    // 500 emoji are 1000 UTF-16 units but 500 code points: allowed.
    expect(() => s.propose("😀".repeat(500))).not.toThrow();
    expect(() => s.propose("😀".repeat(501))).toThrow();
    expect(() => s.propose("   ")).toThrow();
  });

  it("edits, refuses a duplicate, and hard-deletes", () => {
    const s = new MockMemoryStore();
    const a = s.propose("A");
    const b = s.propose("B");
    expect(s.edit(a.id, "A2").content).toBe("A2");
    expect(() => s.edit(b.id, "A2")).toThrow(
      expect.objectContaining({ message: "You already have that memory." }),
    );
    s.delete(a.id);
    expect(s.list().map((m) => m.id)).toEqual([b.id]);
    expect(() => s.delete(a.id)).toThrow(
      expect.objectContaining({ code: "NOT_FOUND" }),
    );
  });
});

describe("rememberText", () => {
  it("keeps the text exactly and needs 'that' or a colon", () => {
    expect(rememberText("Remember that my sister's name is Anaïs.")).toBe(
      "my sister's name is Anaïs.",
    );
    expect(rememberText("  please REMEMBER THAT I like Tea  ")).toBe(
      "I like Tea",
    );
    expect(rememberText("remember: call mum")).toBe("call mum");
    for (const q of ["remember", "remember when we met", "remember that"]) {
      expect(rememberText(q), q).toBeNull();
    }
  });
});
