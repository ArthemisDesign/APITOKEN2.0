import { NextResponse, type NextRequest } from "next/server";
import { resolveShortReferral } from "@/lib/crm-referral-gateway";

export async function proxy(request: NextRequest) {
  const code = request.nextUrl.pathname.slice(1);
  const destination = await resolveShortReferral(code);
  if (!destination) {
    return new NextResponse(null, {
      status: 404,
      headers: {
        "cache-control": "no-store",
        "referrer-policy": "no-referrer",
      },
    });
  }

  const response = NextResponse.redirect(destination, 303);
  response.headers.set("cache-control", "no-store");
  response.headers.set("referrer-policy", "no-referrer");
  return response;
}

export const config = {
  matcher: ["/:referralCode([0-9][a-z0-9]{6})"],
};
