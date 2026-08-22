import { describe, expect, it } from "vitest";
import {
  clampPageOffset,
  compactCapacityUrl,
  compactOverviewUrl,
  ENGINE_ACCOUNT_PAGE,
  pagedOverviewUrl,
} from "./engine-urls";

describe("engine panel URLs", () => {
  it("asks the engine for compact calibration turns and a zero-length account list", () => {
    expect(compactCapacityUrl()).toBe("/capacity?recent_turns=0");
    expect(compactOverviewUrl()).toBe("/overview?accounts_limit=0");
    expect(pagedOverviewUrl(50)).toBe(`/overview?accounts_limit=${ENGINE_ACCOUNT_PAGE}&accounts_offset=50`);
  });

  it("clamps a page offset onto the last full page when the list shrinks", () => {
    expect(clampPageOffset(0, 120, 50)).toBe(0);
    expect(clampPageOffset(150, 120, 50)).toBe(100);
    expect(clampPageOffset(50, 0, 50)).toBe(50);
  });
});
