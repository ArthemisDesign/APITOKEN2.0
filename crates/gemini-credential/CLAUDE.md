# `gemini-credential` — локальный контракт

Этот крейт владеет только форматом и проверкой зашифрованных Gemini OAuth-конвертов, pending
secret-конвертов и канонизацией прокси. Здесь нет HTTP, внешней сети, env, БД, roster-I/O или
логики Auth Bot/runtime; производитель и потребители передают готовые значения.

Критические инварианты:

1. Google identity, email, project, OAuth material и authenticated proxy существуют только внутри
   XChaCha20-Poly1305 envelope. `Debug`, ошибки и тестовые snapshots не раскрывают plaintext.
2. Version, `kid`, profile/context id в AAD, pinned OAuth identity/token endpoint, bounded fields,
   key rotation и zeroization остаются fail-closed. Изменение wire-формата требует явной версии.
3. План принимается только по reviewed tier evidence. Точный известный tier ID — authority и
   переживает изменение display name; точное известное имя другого плана конфликтует и отклоняется.
   Неизвестный ID или знакомая подстрока (`Pro`, `Ultra`) сами по себе доступ не дают.
4. Прокси канонизируется обратимо: percent-encoded userinfo декодируется один раз и кодируется в
   unreserved-набор. Нельзя логировать, возвращать в ошибке или ослаблять проверку origin/path.
5. Файловую атомарность, permissions, symlink/path guards и roster publication реализуют владельцы
   I/O (`authbot`/runtime); этот крейт предоставляет чистые encode/decode/validate primitives.

Проверка: `cargo test -p gemini-credential`. При изменении plan/tier validation обязательно также
запустить `cargo test -p authbot`, потому что Auth Bot использует этот allowlist до публикации.
