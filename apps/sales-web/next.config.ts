import type { NextConfig } from "next";

// CSP для партнёрского портала. Единственный внешний контур — Telegram Login Widget:
// его скрипт грузится с telegram.org, а сам виджет встраивает iframe на oauth.telegram.org.
// API (sales-api) — same-origin через Caddy на partners.apitoken.sale. Inline-скрипты/стили
// обязательны: Next 16 встраивает inline-скрипты, а UI использует inline style-атрибуты.
const csp = [
  "default-src 'self'",
  "script-src 'self' 'unsafe-inline' https://telegram.org",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data:",
  "font-src 'self'",
  "connect-src 'self'",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'self'",
  "frame-src https://oauth.telegram.org",
  "frame-ancestors 'none'",
].join("; ");

const nextConfig: NextConfig = {
  poweredByHeader: false,
  async headers() {
    return [
      {
        source: "/:path*",
        headers: [
          { key: "Content-Security-Policy", value: csp },
          { key: "X-Content-Type-Options", value: "nosniff" },
          { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
          { key: "Permissions-Policy", value: "camera=(), microphone=(), geolocation=()" },
        ],
      },
    ];
  },
};

export default nextConfig;
