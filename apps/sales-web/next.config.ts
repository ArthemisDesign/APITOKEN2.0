import type { NextConfig } from "next";

// Security-заголовки, одинаковые для всего портала. CSP здесь НЕТ: она зависит от маршрута
// (страницам входа нужен Telegram Login Widget) и выставляется в src/proxy.ts из src/lib/csp.ts.
// Два пересекающихся правила headers() дали бы два заголовка CSP, а браузер применяет их
// пересечение — вход снова оказался бы заблокирован.
const nextConfig: NextConfig = {
  poweredByHeader: false,
  async headers() {
    return [
      {
        source: "/:path*",
        headers: [
          { key: "X-Content-Type-Options", value: "nosniff" },
          { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
          { key: "Permissions-Policy", value: "camera=(), microphone=(), geolocation=()" },
        ],
      },
    ];
  },
};

export default nextConfig;
