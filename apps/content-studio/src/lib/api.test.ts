import { describe, expect, it } from "vitest";
import { apiErrorMessage } from "./api";

describe("Content Studio API errors", () => {
  it("renders structured field validation errors", () => {
    expect(apiErrorMessage({
      message: { formErrors: [], fieldErrors: { profiles: ["Invalid"] } },
    }, 400)).toBe("profiles: Invalid");
  });

  it("falls back to the HTTP status when the response has no useful message", () => {
    expect(apiErrorMessage(null, 503)).toBe("Request failed (503)");
  });
});
