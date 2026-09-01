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

  /* sidebar scroll-spy: highlights the nav link of the section in view */
  function initScrollSpy(){
    const nav = document.getElementById('docsNav');
    if(!nav) return;
    const links = Array.prototype.slice.call(nav.querySelectorAll('a'));
    const byId = {};
    const sections = [];
    links.forEach(function(a){
      const id = a.getAttribute('href').slice(1);
      const el = document.getElementById(id);
      if(el){ byId[id] = a; sections.push(el); }
    });
    if(!sections.length) return;

    const observer = new IntersectionObserver(function(entries){
      entries.forEach(function(entry){
        if(entry.isIntersecting){
          links.forEach(function(a){ a.classList.remove('active'); });
          const link = byId[entry.target.id];
          if(link){
            link.classList.add('active');
            /* keep the active link visible inside the scrollable sidebar */
            if(link.scrollIntoView){
              const box = nav.parentElement.getBoundingClientRect();
              const r = link.getBoundingClientRect();
              if(r.top < box.top || r.bottom > box.bottom){
                link.scrollIntoView({ block: 'nearest' });
              }
            }
          }
        }
      });
    }, { rootMargin: '-15% 0px -75% 0px' });

    sections.forEach(function(el){ observer.observe(el); });
  }

  /* smooth anchor scrolling; on mobile an open sidebar closes on navigate */
  function initAnchors(){
    document.querySelectorAll('a[href^="#"]').forEach(function(a){
      a.addEventListener('click', function(e){
        const id = a.getAttribute('href').slice(1);
        const el = document.getElementById(id);
        if(!el) return;
        e.preventDefault();
        document.body.classList.remove('docs-nav-open');
        const header = document.getElementById('hdr');
        const offset = (header ? header.offsetHeight : 0) + 16;
        const top = el.getBoundingClientRect().top + window.scrollY - offset;
        window.scrollTo({ top: top, behavior: 'smooth' });
        history.replaceState(null, '', '#' + id);
      });
    });
  }

  /* mobile: burger toggles the off-canvas sidebar */
  function initSideToggle(){
    const btn = document.getElementById('docsSideTgl');
    if(!btn) return;
    btn.addEventListener('click', function(){
      const open = document.body.classList.toggle('docs-nav-open');
      btn.setAttribute('aria-expanded', open ? 'true' : 'false');
    });
  }

  /* copy-to-clipboard for [data-copy] buttons and the "copy page" button */
  function initCopy(){
    document.querySelectorAll('[data-copy]').forEach(function(btn){
      btn.addEventListener('click', function(){
        const text = btn.getAttribute('data-copy');
        if(!text) return;
        copyText(text).then(function(){ flash(btn); });
      });
    });

    const pageBtn = document.querySelector('.docs-copy-page');
    if(pageBtn){
      pageBtn.addEventListener('click', function(){
        const url = pageBtn.getAttribute('data-copy-url');
        const fallback = function(){ copyText(document.body.innerText).then(function(){ flash(pageBtn); }); };
        if(!url){ fallback(); return; }
        fetch(url)
          .then(function(r){ if(!r.ok) throw new Error('bad'); return r.text(); })
          .then(function(md){ return copyText(md); })
          .then(function(){ flash(pageBtn); })
          .catch(fallback);
      });
    }

    function copyText(text){
      if(navigator.clipboard && navigator.clipboard.writeText){
        return navigator.clipboard.writeText(text);
      }
      return new Promise(function(resolve){
        const ta = document.createElement('textarea');
        ta.value = text;
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.select();
        try { document.execCommand('copy'); } catch(e) {}
        document.body.removeChild(ta);
        resolve();
      });
    }

    function flash(btn){
      if(btn.classList.contains('copied')) return;
      const label = btn.textContent;
      const isIcon = btn.classList.contains('docs-agent-chip-btn');
      btn.classList.add('copied');
      if(!isIcon) btn.textContent = 'Скопировано';
      window.setTimeout(function(){
        btn.classList.remove('copied');
        if(!isIcon) btn.textContent = label;
      }, 1400);
    }
  }

  function init(){
    initThemeToggle();
    initDocsTabs();
    initScrollSpy();
    initAnchors();
    initSideToggle();
    initCopy();
  }

  if(document.readyState === 'loading'){
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();

/* language switcher: glide the thumb to the hovered/target language on click,
   then let the plain link navigate — no page fade, no white flash */
(function(){
  document.querySelectorAll('.lang').forEach(function(wrap){
    wrap.querySelectorAll('.lang__link').forEach(function(a){
      a.addEventListener('click', function(){
        wrap.dataset.active = /en\.html/.test(a.getAttribute('href') || '') ? 'en' : 'ru';
      });
    });
  });
})();
