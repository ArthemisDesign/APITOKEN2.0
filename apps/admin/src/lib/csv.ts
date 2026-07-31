// CSV текущей загруженной страницы — порт downloadCsv/csvDate из admin-panel.js
// (строки 76-82): разделитель ';' (Excel-RU), экранирование кавычек/разделителей/
// переводов строк по RFC 4180, BOM в начале — чтобы Excel открыл UTF-8 без
// «кракозябр». Без библиотек.

// Экранирование ячейки по RFC 4180: есть '"', ';', '\n' или '\r' → обернуть
// в кавычки, внутренние кавычки удвоить.
export function csvCell(value: unknown): string {
  const text = String(value ?? "");
  return /[";\n\r]/.test(text) ? '"' + text.replace(/"/g, '""') + '"' : text;
}

// Весь CSV целиком: строки через CRLF, BOM (\uFEFF) в начале.
export function buildCsv(header: unknown[], rows: unknown[][]): string {
  return "\uFEFF" + [header, ...rows].map((row) => row.map(csvCell).join(";")).join("\r\n");
}

// Скачивание через Blob + временная ссылка. Только браузер.
export function downloadCsv(filename: string, header: unknown[], rows: unknown[][]): void {
  const link = document.createElement("a");
  link.href = URL.createObjectURL(new Blob([buildCsv(header, rows)], { type: "text/csv;charset=utf-8" }));
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(link.href), 1000);
}

// Дата для имени файла: "2026-07-31".
export function csvDate(): string {
  return new Date().toISOString().slice(0, 10);
}
