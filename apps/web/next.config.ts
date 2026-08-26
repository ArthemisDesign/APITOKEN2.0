import type { NextConfig } from "next";

const legacyPages = [
  "plans", "models", "docs", "login", "register", "dashboard", "integrations",
  "int-claude-code", "int-cursor", "int-cline", "int-continue", "int-zed", "int-sdk",
  "terms", "privacy",
];

// Retired direct paths remain redirect-only aliases, so old bookmarks reach the dashboard
// instead of a 404. They are not active sections and never appear in navigation or UI state.
const legacyDashboardPaths = [
  "overview", "keys", "credits", "promos", "usage", "profile", "security",
];

// Короткие ссылки для шеринга: /go/<slug> → главная с UTM. 302 (не permanent), чтобы можно было
// перенацелить slug и чтобы браузеры/мессенджеры не закешировали редирект навсегда.
const shortLinks: Record<string, string> = {
  vibe: "/?utm_source=telegram&utm_medium=community&utm_campaign=vibe_code_community",
};

// The web/v2 Vercel preview is a dashboard-only design environment. The production
// backend accepts the production browser origin, so preview builds use local stateful
// fixtures and open as an already authenticated account. Production builds never set
// this flag and keep the normal authentication flow.
const previewFixtures = process.env.VERCEL_ENV === "preview" || process.env.NEXT_PUBLIC_PREVIEW_FIXTURES === "1";

// CSP для клиентского фронта. Public-поверхность: Yandex Metrika (инлайн-бутстрап +
// внешний скрипт mc.yandex.ru и его beacon) и API на backend.apitoken.sale
// (NEXT_PUBLIC_BACKEND_URL). Всё остальное — same-origin; checkout-редирект на платёжки
// идёт через window.location (навигация, не form/frame), поэтому в CSP он не нужен.
// Inline-скрипты/стили обязательны: Next 16 встраивает inline-скрипты, а layout использует
// dangerouslySetInnerHTML для темы/referral/Metrika, плюс inline style-атрибуты в UI.
const csp = [
  "default-src 'self'",
  "script-src 'self' 'unsafe-inline' https://mc.yandex.ru",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: https://mc.yandex.ru",
  "font-src 'self'",
  "connect-src 'self' https://backend.apitoken.sale https://mc.yandex.ru",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'self'",
  "frame-ancestors 'none'",
].join("; ");

// Landing + dashboard build. Static landing is served from public/landing via rewrites.
const nextConfig: NextConfig = {
  poweredByHeader: false,
  env: { NEXT_PUBLIC_PREVIEW_FIXTURES: previewFixtures ? "1" : "" },
  async redirects() {
    return [
      ...(previewFixtures ? [
        { source: "/login", destination: "/dashboard", permanent: false },
        { source: "/register", destination: "/dashboard", permanent: false },
        { source: "/forgot-password", destination: "/dashboard", permanent: false },
        { source: "/reset-password", destination: "/dashboard", permanent: false },
        { source: "/verify-email", destination: "/dashboard", permanent: false },
        { source: "/auth/callback", destination: "/dashboard", permanent: false },
        { source: "/ru", destination: "/ru/dashboard", permanent: false },
        { source: "/ru/login", destination: "/ru/dashboard", permanent: false },
        { source: "/ru/register", destination: "/ru/dashboard", permanent: false },
        { source: "/ru/forgot-password", destination: "/ru/dashboard", permanent: false },
        { source: "/ru/reset-password", destination: "/ru/dashboard", permanent: false },
        { source: "/ru/verify-email", destination: "/ru/dashboard", permanent: false },
      ] : []),
      ...legacyPages.map((page) => ({ source: `/${page}.html`, destination: `/${page}`, permanent: true })),
      ...legacyDashboardPaths.map((section) => ({ source: `/dashboard/${section}`, destination: "/dashboard", permanent: false })),
      ...Object.entries(shortLinks).map(([slug, destination]) => ({ source: `/go/${slug}`, destination, permanent: false })),
      // /e/<code> → справочник ошибок. Код едет ФРАГМЕНТОМ, а не query-параметром:
      // параметр породил бы второй сканируемый URL той же страницы (дубль), а фрагмент
      // поисковики отдельным URL не считают и браузер сам скроллит к секции.
      // Учёт по коду всё равно работает — ErrorAnchorBeacon читает location.hash
      // (Метрика срезает хеш только из отчётного URL, но не из того, что видит JS).
      { source: "/e/:code", destination: "/docs/errors#e-:code", permanent: false },
    ];
  },
  async rewrites() {
    // The static marketing landing lives in public/landing and is served at / and /landing
    // without changing the browser URL.
    return [
      { source: "/", destination: "/landing/index.html" },
      { source: "/landing", destination: "/landing/index.html" },
    ];
  },
  async headers() {
    return [{
      source: "/:path*",
      headers: [
        { key: "Content-Security-Policy", value: csp },
        { key: "X-Content-Type-Options", value: "nosniff" },
        { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
        { key: "Permissions-Policy", value: "camera=(), microphone=(), geolocation=()" },
      ],
    }];
  },
};

export default nextConfig;
