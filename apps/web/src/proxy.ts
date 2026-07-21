import { NextResponse, type NextRequest } from "next/server";
import { documentLanguageForPathname } from "@/lib/locale-routes";

export function proxy(request: NextRequest) {
  const requestHeaders = new Headers(request.headers);
  requestHeaders.set("x-document-language", documentLanguageForPathname(request.nextUrl.pathname));
  return NextResponse.next({ request: { headers: requestHeaders } });
}

// Only document routes need locale propagation. Static assets and API traffic
// bypass this lightweight request transform.
export const config = {
  matcher: ["/((?!v1(?:/|$)|_next/static|_next/image|.*\\..*).*)"],
};
