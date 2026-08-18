import { NextResponse, type NextRequest } from "next/server";
import { contentSecurityPolicy } from "@/lib/csp";

// CSP выдаётся здесь, а не в next.config.ts: политика зависит от маршрута
// (страницам входа нужен Telegram Login Widget), а конфиг статичен. Остальные
// security-заголовки остаются в next.config.ts — они одинаковы для всего портала.
export default function proxy(request: NextRequest): NextResponse {
  const response = NextResponse.next();
  response.headers.set("Content-Security-Policy", contentSecurityPolicy(request.nextUrl.pathname));
  return response;
}

export const config = {
  // Статика и шрифты отдаются без CSP — политика имеет смысл только для документов.
  matcher: ["/((?!_next/static|_next/image|assets/|favicon.ico).*)"],
};
