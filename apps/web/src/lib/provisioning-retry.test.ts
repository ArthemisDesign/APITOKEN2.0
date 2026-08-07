import { describe, expect, it } from "vitest";
import { ApiError } from "./api";
import { withProvisioningRetry } from "./provisioning-retry";

describe("withProvisioningRetry", () => {
  it("waits out the provisioning window and returns the first successful answer", async () => {
    let calls = 0;
    const result = await withProvisioningRetry(async () => {
      calls += 1;
      if (calls < 3) throw new ApiError("engine is temporarily unavailable", 503);
      return { keys: [] };
    }, { attempts: 4, delayMs: 0 });

    expect(result).toEqual({ keys: [] });
    expect(calls).toBe(3);
  });

  it("gives up after the bounded wait so a lasting outage still surfaces", async () => {
    let calls = 0;
    const failure = await withProvisioningRetry(async () => {
      calls += 1;
      throw new ApiError("engine is temporarily unavailable", 503);
    }, { attempts: 2, delayMs: 0 }).catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect(calls).toBe(3);
  });

  it("never retries a status that is not a retry instruction", async () => {
    for (const status of [401, 409, 500, 502]) {
      let calls = 0;
      const failure = await withProvisioningRetry(async () => {
        calls += 1;
        throw new ApiError("nope", status);
      }, { attempts: 4, delayMs: 0 }).catch((error: unknown) => error);

      expect(failure).toMatchObject({ status });
      expect(calls).toBe(1);
    }
  });

  it("propagates a non-API failure (offline, aborted fetch) immediately", async () => {
    let calls = 0;
    const failure = await withProvisioningRetry(async () => {
      calls += 1;
      throw new TypeError("Failed to fetch");
    }, { attempts: 4, delayMs: 0 }).catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(TypeError);
    expect(calls).toBe(1);
  });
});
