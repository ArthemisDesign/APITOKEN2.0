// migrate.js намеренно НЕ реэкспортируется: это CLI-энтрипоинт деплоя
// (node dist/migrate.js), а его ссылка на каталог ../migrations ломает
// сборку приложения, если попадает в бандл.
export * from "./client.js";
export * from "./schema.js";
