// Копирует текст thead в data-label ячеек, чтобы на телефоне CSS сложил
// каждую строку в карточку с подписью колонки. Не трогает empty/colspan
// (развёрнутый usage) и уже проставленные data-label.
export function normalizeTableHead(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

export function stampTableLabels(root: ParentNode): number {
  let stamped = 0;
  for (const table of root.querySelectorAll("table")) {
    const heads = [...table.querySelectorAll("thead th")].map((th) => normalizeTableHead(th.textContent ?? ""));
    if (!heads.some(Boolean)) continue;
    for (const row of table.querySelectorAll(":scope > tbody > tr")) {
      if (row.querySelector(":scope > td.empty, :scope > td[colspan]")) continue;
      const cells = [...row.children].filter((node) => node.tagName === "TD");
      cells.forEach((cell, index) => {
        if (!(cell instanceof HTMLElement) || cell.hasAttribute("data-label") || !heads[index]) return;
        cell.setAttribute("data-label", heads[index]);
        stamped += 1;
      });
    }
  }
  return stamped;
}
