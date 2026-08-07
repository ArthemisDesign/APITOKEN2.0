import { ApiError } from "./api";

// A brand-new account provisions its engine mapping and pricing policy on the very first
// dashboard load, and the section requests race each other through it: whichever loses gets a
// truthful "temporarily unavailable" 503 that resolves within seconds. Showing the
// "could not be loaded" notice for that window tells a first-time customer their account is
// broken when it is merely a second old. Wait it out behind the skeleton instead. Only 503 is
// waited on: it is the one status the API uses for "retry this exact request shortly". Any other
// failure, and an exhausted wait, propagate unchanged so a real outage still surfaces.
export const PROVISIONING_RETRY_ATTEMPTS = 4;
export const PROVISIONING_RETRY_DELAY_MS = 700;

export async function withProvisioningRetry<T>(
  action: () => Promise<T>,
  options: { attempts?: number; delayMs?: number } = {},
): Promise<T> {
  const attempts = options.attempts ?? PROVISIONING_RETRY_ATTEMPTS;
  const delayMs = options.delayMs ?? PROVISIONING_RETRY_DELAY_MS;
  for (let attempt = 0; ; attempt += 1) {
    try {
      return await action();
    } catch (cause) {
      if (!(cause instanceof ApiError) || cause.status !== 503 || attempt >= attempts) throw cause;
      await new Promise((resolve) => setTimeout(resolve, delayMs));
    }
  }
}
