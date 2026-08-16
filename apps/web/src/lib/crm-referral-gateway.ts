const SHORT_REFERRAL_CODE = /^[0-9][a-z0-9]{6}$/;
const CRM_REFERRAL_ORIGIN = "https://crm.apitoken.sale";
const PUBLIC_SITE_ORIGIN = "https://apitoken.sale";

const isRedirectStatus = (status: number): boolean => status === 302 || status === 303;

export function isShortReferralCode(value: string): boolean {
  return SHORT_REFERRAL_CODE.test(value);
}

export function safeReferralDestination(value: string | null): string | null {
  if (!value) return null;

  try {
    const destination = new URL(value);
    const referrals = destination.searchParams.getAll("ref");
    const contents = destination.searchParams.getAll("utm_content");
    if (
      destination.origin !== PUBLIC_SITE_ORIGIN
      || destination.username !== ""
      || destination.password !== ""
      || destination.pathname !== "/"
      || destination.hash !== ""
      || referrals.length !== 1
      || contents.length !== 1
      || referrals[0] === ""
      || referrals[0] !== contents[0]
    ) {
      return null;
    }
    return destination.toString();
  } catch {
    return null;
  }
}

export async function resolveShortReferral(
  code: string,
  fetcher: typeof fetch = fetch,
): Promise<string | null> {
  if (!isShortReferralCode(code)) return null;

  try {
    const response = await fetcher(`${CRM_REFERRAL_ORIGIN}/r/${code}`, {
      method: "GET",
      cache: "no-store",
      redirect: "manual",
      headers: { accept: "text/html" },
      signal: AbortSignal.timeout(4_000),
    });
    if (!isRedirectStatus(response.status)) return null;
    return safeReferralDestination(response.headers.get("location"));
  } catch {
    return null;
  }
}
