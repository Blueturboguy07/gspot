import { describe, expect, it } from "vitest";
import { initials, keyKind, relativeTime, wordCount, wordDiff } from "./text";

describe("keyKind", () => {
  it("names the provider from the key prefix", () => {
    expect(keyKind("sk-ant-api03-xyz")).toBe("Anthropic key");
    expect(keyKind("sk-or-v1-xyz")).toBe("OpenRouter key");
    expect(keyKind("gsk_xyz")).toBe("Groq key");
    expect(keyKind("AIzaSyXYZ")).toBe("Google AI key");
    expect(keyKind("sk-proj-xyz")).toBe("OpenAI key");
    expect(keyKind("  ")).toBe("");
    expect(keyKind("whatever")).toBe("API key");
  });
});

describe("initials", () => {
  it("takes the first two words", () => {
    expect(initials("Mann Bellani")).toBe("MB");
    expect(initials("mann")).toBe("M");
    expect(initials("")).toBe("G");
  });
});

describe("wordDiff", () => {
  it("marks insertions and deletions at word level", () => {
    const ops = wordDiff("I has a cat", "I have a cat");
    expect(ops.map((o) => o.kind)).toEqual(["same", "del", "ins", "same"]);
    expect(ops[1]!.text).toBe("has");
    expect(ops[2]!.text).toBe("have");
  });
  it("returns a single same op for identical text", () => {
    expect(wordDiff("a b", "a b")).toEqual([{ kind: "same", text: "a b" }]);
  });
});

describe("wordCount + relativeTime", () => {
  it("counts unicode words", () => {
    expect(wordCount("Hello, world! It's 3 o'clock")).toBe(5);
  });
  it("formats relative time", () => {
    expect(relativeTime(1000, 1030)).toBe("just now");
    expect(relativeTime(1000, 1000 + 600)).toBe("10 min ago");
    expect(relativeTime(1000, 1000 + 7200)).toBe("2 h ago");
  });
});
