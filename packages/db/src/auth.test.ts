import { describe, expect, it } from "vitest";
import { initialDisplayName } from "./auth.js";

describe("initial display names", () => {
  it("uses a bounded provider name when it is valid", () => {
    expect(initialDisplayName("developer@example.com", `  ${"A".repeat(90)}  `)).toBe("A".repeat(80));
  });

  it("falls back to the email local part for an invalid provider name", () => {
    expect(initialDisplayName("developer@example.com", "Bad\nName")).toBe("developer");
  });
});
