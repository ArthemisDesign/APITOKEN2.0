import { describe, expect, it } from "vitest";
import { isSerializationConflictV2 } from "./pricing-release-orchestration-v2.js";

describe("orchestration serialization classification", () => {
  it("treats PostgreSQL serialization and deadlock outcomes as transient", () => {
    expect(isSerializationConflictV2("could not serialize access due to concurrent update")).toBe(true);
    expect(isSerializationConflictV2("could not serialize access due to read/write dependencies among transactions")).toBe(true);
    expect(isSerializationConflictV2("deadlock detected")).toBe(true);
    expect(isSerializationConflictV2("engine inventory drifted from the exact Stage 5 run")).toBe(false);
    expect(isSerializationConflictV2("engine request timed out")).toBe(false);
  });
});
