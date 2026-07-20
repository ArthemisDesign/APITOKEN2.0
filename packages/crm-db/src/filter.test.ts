import { describe, expect, it } from "vitest";
import { matchesFilter, parseFilter } from "./filter.js";

const attrs = {
  role: "cto",
  geo_country: "UK",
  pain_points: ["api cost", "rate limits"],
  buying_intent: 0.65,
};

describe("parseFilter", () => {
  it("accepts a valid DSL", () => {
    const f = parseFilter({ all: [{ key: "role", op: "eq", value: "cto" }] });
    expect(f.all).toHaveLength(1);
  });

  it("rejects unknown ops and missing values", () => {
    expect(() => parseFilter({ all: [{ key: "role", op: "like", value: "x" }] })).toThrow();
    expect(() => parseFilter({ all: [{ key: "role", op: "eq" }] })).toThrow();
    expect(() => parseFilter({ all: [{ key: "role", op: "exists" }] })).not.toThrow();
  });
});

describe("matchesFilter", () => {
  it("all/any/none semantics", () => {
    expect(
      matchesFilter(attrs, {
        all: [{ key: "role", op: "in", value: ["cto", "founder"] }, { key: "geo_country", op: "eq", value: "uk" }],
        any: [{ key: "buying_intent", op: "gte", value: 0.6 }, { key: "missing", op: "exists" }],
        none: [{ key: "risk_flags", op: "exists" }],
      }),
    ).toBe(true);
    expect(matchesFilter(attrs, { none: [{ key: "role", op: "eq", value: "cto" }] })).toBe(false);
  });

  it("array attributes match element-wise", () => {
    expect(matchesFilter(attrs, { all: [{ key: "pain_points", op: "contains", value: "API Cost" }] })).toBe(true);
    expect(matchesFilter(attrs, { all: [{ key: "pain_points", op: "eq", value: "billing" }] })).toBe(false);
  });

  it("missing keys never match (except via none)", () => {
    expect(matchesFilter(attrs, { all: [{ key: "ghost", op: "exists" }] })).toBe(false);
    expect(matchesFilter(attrs, { none: [{ key: "ghost", op: "exists" }] })).toBe(true);
  });
});
