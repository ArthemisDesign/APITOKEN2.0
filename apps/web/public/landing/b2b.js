/* ---------------------------------------------------------
   apiToken landing — /landing/b2b.html
   App-shell (openkeys-style sidebar, RU only) + key login
   (copy of openkeys key-login.tsx) + B2B form + support.
   --------------------------------------------------------- */
(() => {
  const $ = (s, c = document) => c.querySelector(s);
  const $$ = (s, c = document) => [...c.querySelectorAll(s)];

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

  /* SIDEBAR — burger overlay on mobile (open / scrim / Esc), smooth anchor scroll */
  (() => {
    const side = $('#b2bSide');
    const scrim = $('#b2bScrim');
    const burger = $('#b2bBurger');
    if (!side || !scrim || !burger) return;
    const setOpen = (open) => {
      side.classList.toggle('open', open);
      scrim.classList.toggle('show', open);
      burger.setAttribute('aria-expanded', String(open));
    };
    burger.addEventListener('click', () => setOpen(!side.classList.contains('open')));
    scrim.addEventListener('click', () => setOpen(false));
    addEventListener('keydown', (e) => {
      if (e.key === 'Escape') setOpen(false);
    });
    $$('.b2b-side__link[href^="#"]', side).forEach((link) => {
      link.addEventListener('click', (e) => {
        const target = $(link.getAttribute('href'));
        if (!target) return;
        e.preventDefault();
        setOpen(false);
        target.scrollIntoView({ behavior: 'smooth', block: 'start' });
        history.replaceState(null, '', link.getAttribute('href'));
      });
    });
  })();

  /* SIDEBAR — highlight the anchor item of the section in view */
  (() => {
    const links = $$('.b2b-side__link[data-section]');
    if (!('IntersectionObserver' in window) || !links.length) return;
    const bySection = new Map(links.map((l) => [l.dataset.section, l]));
    const setActive = (id) => {
      links.forEach((l) => {
        const on = l.dataset.section === id;
        l.classList.toggle('is-active', on);
        if (on) l.setAttribute('aria-current', 'page'); else l.removeAttribute('aria-current');
      });
    };
    const visible = new Map(); // section id -> intersection ratio
    const io = new IntersectionObserver((entries) => {
      entries.forEach((en) => {
        if (en.isIntersecting) visible.set(en.target.id, en.intersectionRatio);
        else visible.delete(en.target.id);
      });
      let best = null;
      visible.forEach((ratio, id) => { if (!best || ratio > best[1]) best = [id, ratio]; });
      if (best && bySection.has(best[0])) setActive(best[0]);
    }, { rootMargin: '-10% 0px -55% 0px', threshold: [0, 0.1, 0.4, 0.8] });
    ['usage', 'offer', 'support'].forEach((id) => { const el = document.getElementById(id); if (el) io.observe(el); });
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
