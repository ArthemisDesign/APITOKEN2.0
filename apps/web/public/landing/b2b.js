/* ---------------------------------------------------------
   apiToken landing — /landing/b2b.html + /landing/b2b-en.html
   Sidebar in the docs composition (brand / label / mono nav /
   lang + theme in the foot), floating burger off-canvas on
   mobile. Key login and support texts mirror apps/openkeys
   (key-login.tsx, support-portal.tsx); RU/EN strings live in
   data-* attributes of the forms on each page.
   --------------------------------------------------------- */
(() => {
  const $ = (s, c = document) => c.querySelector(s);
  const $$ = (s, c = document) => [...c.querySelectorAll(s)];

  /* THEME TOGGLE — light (warm paper) ↔ dark (deep ink), persisted.
     Same handler as docs.js; the button sits in the sidebar foot. */
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

  /* SIDEBAR — floating burger opens it off-canvas on mobile (body class,
     like body.docs-nav-open in the docs), scrim/Esc close, smooth anchors */
  (() => {
    const burger = $('#b2bBurger');
    const scrim = $('#b2bScrim');
    if (!burger) return;
    const setOpen = (open) => {
      document.body.classList.toggle('b2b-nav-open', open);
      scrim?.classList.toggle('show', open);
      burger.setAttribute('aria-expanded', String(open));
    };
    burger.addEventListener('click', () => setOpen(!document.body.classList.contains('b2b-nav-open')));
    scrim?.addEventListener('click', () => setOpen(false));
    addEventListener('keydown', (e) => {
      if (e.key === 'Escape') setOpen(false);
    });
    $$('.b2b-side__link[href^="#"]').forEach((link) => {
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

  /* KEY LOGIN — same flow as apps/openkeys key-login.tsx, texts come from
     the form's data-* attributes so one script serves the RU and EN pages.
     The session cookie and the dashboard both live on openkeys.apitoken.sale. */
  (() => {
    const form = $('#keyForm');
    if (!form) return;
    const input = $('#apikey');
    const btn = form.querySelector('button[type="submit"]');
    const err = $('#keyError');
    const copy = {
      missing: form.dataset.errorMissing,
      unavailable: form.dataset.errorUnavailable,
      checking: form.dataset.checking,
      submit: form.dataset.submit,
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

  /* B2B FORM — client-only, same pattern as #b2bForm in app.js;
     the "sent" label is page-localized via data-sent */
  $('#b2bForm')?.addEventListener('submit', (e) => {
    e.preventDefault();
    $('#b2bOk').hidden = false;
    e.target.querySelector('button').textContent = e.target.dataset.sent;
  });
})();

/* language switcher: glide the thumb to the target language on click,
   then let the plain link navigate — same as docs.js */
(function(){
  document.querySelectorAll('.lang').forEach(function(wrap){
    wrap.querySelectorAll('.lang__link').forEach(function(a){
      a.addEventListener('click', function(){
        wrap.dataset.active = /en\.html/.test(a.getAttribute('href') || '') ? 'en' : 'ru';
        wrap.querySelectorAll('.lang__link').forEach(function(x){ x.classList.toggle('is-active', x === a); });
      });
    });
  });
})();
