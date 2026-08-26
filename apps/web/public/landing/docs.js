(function(){
  'use strict';

  /* theme toggle — light (warm paper) ↔ dark (deep ink), persisted.
     Mirrors the handler in app.js so Docs pages switch theme too. */
  function initThemeToggle(){
    const KEY = 'apitoken-theme';
    const root = document.documentElement;
    const btn = document.getElementById('themeTgl');
    const apply = (t) => {
      if (t === 'dark') root.dataset.theme = 'dark'; else delete root.dataset.theme;
      if (btn) btn.textContent = t === 'dark' ? '☾' : '☀';
    };
    let saved = 'light';
    try { saved = localStorage.getItem(KEY) === 'dark' ? 'dark' : 'light'; } catch {}
    apply(saved);
    if (btn) btn.addEventListener('click', () => {
      saved = root.dataset.theme === 'dark' ? 'light' : 'dark';
      try { localStorage.setItem(KEY, saved); } catch {}
      apply(saved);
    });
  }

  function initDocsTabs(){
    const tabs = document.querySelectorAll('.docs-tabs .tab');
    const panels = document.querySelectorAll('.docs-panel');
    if(!tabs.length) return;

    tabs.forEach(function(tab){
      tab.addEventListener('click', function(){
        const target = tab.dataset.tab;

        tabs.forEach(function(t){ t.classList.remove('on'); });
        tab.classList.add('on');

        panels.forEach(function(p){
          p.classList.toggle('active', p.dataset.panel === target);
        });
      });
    });
  }

  function init(){
    initThemeToggle();
    initDocsTabs();
  }

  if(document.readyState === 'loading'){
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
