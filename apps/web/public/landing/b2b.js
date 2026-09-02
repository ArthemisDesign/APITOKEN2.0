/* ---------------------------------------------------------
   apiToken landing — /landing/b2b.html
   Key login (copy of openkeys key-login.tsx) + B2B form.
   --------------------------------------------------------- */
(() => {
  const $ = (s, c = document) => c.querySelector(s);

  /* HEADER — shrink on scroll + burger, same contract as app.js */
  const hdr = $('#hdr');
  if (hdr) addEventListener('scroll', () => hdr.classList.toggle('small', scrollY > 40), { passive: true });
  $('#burger')?.addEventListener('click', () => $('.nav')?.classList.toggle('open'));

  /* THEME TOGGLE — light (warm paper) ↔ dark (deep ink), persisted */
  (() => {
    const KEY = 'apitoken-theme';
    const root = document.documentElement;
    const btn = $('#themeTgl');
    const apply = (t) => {
      if (t === 'dark') root.dataset.theme = 'dark'; else delete root.dataset.theme;
      if (btn) btn.textContent = t === 'dark' ? '☾' : '☀';
    };
    let saved = 'light';
    try { saved = localStorage.getItem(KEY) === 'dark' ? 'dark' : 'light'; } catch {}
    apply(saved);
    btn?.addEventListener('click', () => {
      saved = root.dataset.theme === 'dark' ? 'light' : 'dark';
      try { localStorage.setItem(KEY, saved); } catch {}
      apply(saved);
    });
  })();

  /* KEY LOGIN — same flow and texts as apps/openkeys key-login.tsx (ru copy),
     but against the live openkeys API: the session cookie and the dashboard
     both live on openkeys.apitoken.sale. */
  (() => {
    const form = $('#keyForm');
    if (!form) return;
    const input = $('#apikey');
    const btn = form.querySelector('button[type="submit"]');
    const err = $('#keyError');
    const copy = {
      missing: 'Ключ не найден. Проверьте, что скопировали его целиком.',
      unavailable: 'Не удалось связаться с сервером. Попробуйте ещё раз.',
      checking: 'Проверяем…',
      submit: 'Открыть USAGE',
    };
    const sync = () => { btn.disabled = input.value.trim() === ''; };
    input.addEventListener('input', sync);
    sync();
    form.addEventListener('submit', async (e) => {
      e.preventDefault();
      btn.disabled = true;
      btn.textContent = copy.checking;
      err.hidden = true;
      try {
        const response = await fetch('https://openkeys.apitoken.sale/api/usage/lookup', {
          method: 'POST',
          cache: 'no-store',
          credentials: 'include',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ key: input.value.trim() }),
        });
        if (!response.ok) {
          err.textContent = copy.missing;
          err.hidden = false;
          return;
        }
        // The server has set the HttpOnly profile session on openkeys.apitoken.sale;
        // the dashboard itself is rendered there.
        window.location.href = 'https://openkeys.apitoken.sale/profile';
      } catch {
        err.textContent = copy.unavailable;
        err.hidden = false;
      } finally {
        btn.textContent = copy.submit;
        sync();
      }
    });
  })();

  /* B2B FORM — client-only, same pattern as #b2bForm in app.js */
  $('#b2bForm')?.addEventListener('submit', (e) => {
    e.preventDefault();
    $('#b2bOk').hidden = false;
    e.target.querySelector('button').textContent = 'Отправлено';
  });
})();
