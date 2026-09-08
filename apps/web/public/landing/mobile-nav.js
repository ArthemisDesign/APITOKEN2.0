/* Shared mobile navigation for the landing, docs and openKeys pages. */
(() => {
  // Match browser chrome to the actual header, not the OS theme. In particular,
  // the notch/status area must not leave a paper or transparent strip over coral.
  if (document.documentElement.classList.contains('landing-home')) {
    // Suppress boundary rubber-banding/pull-to-refresh on touch browsers that
    // do not honour root overscroll-behavior. Normal scrolling, inner scrollers,
    // horizontal gestures, form controls and pinch zoom remain native.
    let touch = null;
    document.addEventListener('touchstart', event => {
      touch = event.touches.length === 1 ? {x:event.touches[0].clientX,y:event.touches[0].clientY} : null;
    }, {passive:true});
    document.addEventListener('touchmove', event => {
      if (!touch || event.touches.length !== 1) { touch = null; return; }
      const point = event.touches[0];
      const dx = point.clientX - touch.x, dy = point.clientY - touch.y;
      touch = {x:point.clientX,y:point.clientY};
      if (!event.cancelable || Math.abs(dy) <= Math.abs(dx) || !dy) return;
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest('input,textarea,select,[contenteditable="true"]')) return;
      const root = document.scrollingElement;
      if (!root) return;
      for (let node = target; node && node !== document.body && node !== root; node = node.parentElement) {
        if (!/^(auto|scroll)$/.test(getComputedStyle(node).overflowY)) continue;
        const remaining = node.scrollHeight - node.clientHeight;
        if (remaining > 1 && (dy > 0 ? node.scrollTop > 0 : node.scrollTop < remaining - 1)) return;
      }
      const atTop = root.scrollTop <= 0;
      const atBottom = root.scrollTop >= root.scrollHeight - root.clientHeight - 1;
      if ((dy > 0 && atTop) || (dy < 0 && atBottom)) event.preventDefault();
    }, {passive:false});
    const clearTouch = () => { touch = null; };
    document.addEventListener('touchend', clearTouch, {passive:true});
    document.addEventListener('touchcancel', clearTouch, {passive:true});
    const header = document.getElementById('hdr');
    const themeColor = document.querySelector('meta[name="theme-color"]');
    const syncChrome = () => {
      if (header && themeColor) themeColor.content = getComputedStyle(header).backgroundColor;
    };
    if (header) {
      new MutationObserver(syncChrome).observe(header, {attributes:true,attributeFilter:['class']});
      new MutationObserver(syncChrome).observe(document.documentElement, {attributes:true,attributeFilter:['data-theme']});
      addEventListener('pageshow', syncChrome);
      syncChrome();
    }
  }
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
