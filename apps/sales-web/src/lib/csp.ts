// Content-Security-Policy для партнёрского портала — единственный источник правды.
//
// Почему это отдельный модуль, а не строка в next.config.ts: политика РАЗНАЯ на двух
// страницах входа и на всём остальном портале, а next.config не умеет отдавать разный
// заголовок для пересекающихся маршрутов. Заголовок ставит src/proxy.ts, тесты — csp.test.ts.
//
// Инцидент, из-за которого политика раздвоилась: Telegram Login Widget
// (https://telegram.org/js/telegram-widget.js) внутри вычисляет код строкой, поэтому под
// строгим script-src он падал с EvalError ДО отрисовки кнопки. Внешне это выглядело как
// «вход не работает»: страница /login рендерилась, но кнопки «Sign in with Telegram» на ней
// не было вообще и нажать было нечего. 'unsafe-eval' выдаём ТОЛЬКО двум страницам входа,
// где нет ни сессии, ни партнёрских данных; кабинет и админка остаются под строгой политикой.

/** Страницы, встраивающие Telegram Login Widget. Только им ослабляем script-src. */
export const TELEGRAM_WIDGET_PATHS = ["/login", "/register"] as const;

export function isTelegramWidgetPath(pathname: string): boolean {
  const normalized = pathname.length > 1 && pathname.endsWith("/") ? pathname.slice(0, -1) : pathname;
  return (TELEGRAM_WIDGET_PATHS as readonly string[]).includes(normalized);
}

export function contentSecurityPolicy(pathname: string): string {
  const widget = isTelegramWidgetPath(pathname);
  return [
    "default-src 'self'",
    // Inline обязателен: Next 16 встраивает inline-скрипты гидрации.
    // 'unsafe-eval' и telegram.org — только на страницах входа (см. шапку файла).
    widget
      ? "script-src 'self' 'unsafe-inline' 'unsafe-eval' https://telegram.org"
      : "script-src 'self' 'unsafe-inline'",
    "style-src 'self' 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self'",
    "connect-src 'self'",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'self'",
    // Виджет открывается iframe'ом с oauth.telegram.org — только на страницах входа.
    widget ? "frame-src https://oauth.telegram.org" : "frame-src 'none'",
    "frame-ancestors 'none'",
  ].join("; ");
}
