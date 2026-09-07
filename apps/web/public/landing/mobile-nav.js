/* Shared mobile navigation for the landing, docs and openKeys pages. */
(() => {
  const configs = [
    ['burger', '#mobNav', 1024, 'landing-nav-open'],
    ['docsSideTgl', '.docs-sidebar', 1024, 'docs-nav-open'],
    ['b2bBurger', '#b2bSide', 900, 'b2b-nav-open'],
  ];
  for (const [id, selector, width, bodyClass] of configs) {
    const button = document.getElementById(id);
    const panel = document.querySelector(selector);
    if (!button || !panel) continue;
    if (!panel.id) panel.id = `${id}-panel`;
    button.setAttribute('aria-controls', panel.id);
    const media = matchMedia(`(max-width:${width}px)`);
    const backdrop = document.getElementById('b2bScrim') || document.createElement('div');
    if (!backdrop.isConnected) {
      backdrop.className = 'mobile-nav-scrim';
      backdrop.setAttribute('aria-hidden', 'true');
      document.body.append(backdrop);
    }
    let open = false;
    let restore = () => {};
    const setOpen = (next, returnFocus = false) => {
      next = next && media.matches;
      if (next !== open) {
        if (next) {
          const bodyOverflow = document.body.style.overflow;
          const htmlOverflow = document.documentElement.style.overflow;
          document.body.style.overflow = 'hidden';
          document.documentElement.style.overflow = 'hidden';
          const content = [...document.querySelectorAll('main,footer')].filter(el => !el.contains(panel));
          const inert = content.map(el => el.inert);
          content.forEach(el => { el.inert = true; });
          restore = () => {
            document.body.style.overflow = bodyOverflow;
            document.documentElement.style.overflow = htmlOverflow;
            content.forEach((el, i) => { el.inert = inert[i]; });
          };
        } else restore();
      }
      open = next;
      document.body.classList.toggle(bodyClass, open);
      panel.classList.toggle('open', open);
      panel.inert = media.matches && !open;
      backdrop.classList.toggle('show', open);
      backdrop.tabIndex = -1;
      button.setAttribute('aria-expanded', String(open));
      if (returnFocus) button.focus({ preventScroll: true });
    };
    button.addEventListener('click', () => setOpen(!open));
    backdrop.addEventListener('click', () => setOpen(false, true));
    panel.addEventListener('click', event => {
      if (event.target.closest('a[href]')) setOpen(false, true);
    });
    document.addEventListener('keydown', event => {
      if (!open) return;
      if (event.key === 'Escape') { event.preventDefault(); setOpen(false, true); }
      if (event.key !== 'Tab') return;
      const items = [button, ...panel.querySelectorAll('a[href],button,input,select,textarea')]
        .filter(el => !el.disabled && el.getClientRects().length);
      const index = items.indexOf(document.activeElement);
      event.preventDefault();
      const next = index < 0 ? (event.shiftKey ? items.length - 1 : 0) : (index + (event.shiftKey ? -1 : 1) + items.length) % items.length;
      items[next]?.focus();
    });
    media.addEventListener('change', () => setOpen(false));
    addEventListener('pageshow', () => setOpen(false));
    setOpen(false);
  }
})();
