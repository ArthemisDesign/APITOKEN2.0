import { redirect } from "next/navigation";

// Отдельной витрины у продукта нет: покупателю нужны только доки, страница
// расхода и (нам) админка. Корень ведёт в доки — это первое, что открывают.
export default function RootPage() {
  redirect("/docs");
}
