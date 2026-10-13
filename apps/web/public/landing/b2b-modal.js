/* =========================================================
   apiToken landing — "Get a B2B quote" pop-up
   Every [data-b2b] link opens the short explainer dialog instead of
   navigating; without JS the link still goes to the B2B page.
   ========================================================= */
(() => {
  const dlg = document.getElementById('b2bDialog');
  if (!dlg || typeof dlg.showModal !== 'function') return;

  const open = () => {
    dlg.showModal();
    if (window.__lenis) window.__lenis.stop();
  };
  dlg.addEventListener('close', () => { if (window.__lenis) window.__lenis.start(); });
  // a click on the backdrop lands on the dialog element itself
  dlg.addEventListener('click', e => { if (e.target === dlg) dlg.close(); });

  document.querySelectorAll('[data-b2b]').forEach(a => a.addEventListener('click', e => {
    e.preventDefault();
    open();
  }));
})();
