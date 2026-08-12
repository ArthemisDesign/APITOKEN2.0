import type { NextConfig } from "next";

const workspaceRoot = new URL("../..", import.meta.url).pathname;

// Админка живёт за Caddy на admin.apitoken.sale: forward_auth и серверные ключи
// внедряет Caddy, само приложение секретов не имеет и ходит в API по same-origin
// относительным путям. CSP без хэшей: Next 16 встраивает inline-скрипты
// (в т.ч. наш скрипт темы), поэтому script-src 'unsafe-inline'.
const csp = [
  "default-src 'self'",
  "script-src 'self' 'unsafe-inline'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data:",
  "font-src 'self'",
  "connect-src 'self'",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'self'",
  "frame-ancestors 'none'",
].join("; ");

const nextConfig: NextConfig = {
  poweredByHeader: false,
  turbopack: { root: workspaceRoot },
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
