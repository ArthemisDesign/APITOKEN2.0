/* =========================================================
   motion.js — GSAP + ScrollTrigger + Lenis + SplitType

   Replaces the hand-rolled version. Everything is scroll-
   driven through one ScrollTrigger instance per effect, and
   scrolling itself is handled by Lenis (real window scroll,
   so position: sticky keeps working).

   Kill switches — declare before this file loads:
     window.MO_SMOOTH = false
     window.MO_GRID   = false
     window.MO_PIN    = false
   ========================================================= */
(async () => {
'use strict';

const $  = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const REDUCED = matchMedia('(prefers-reduced-motion: reduce)').matches;
const TOUCH   = matchMedia('(hover: none)').matches;

if (!window.gsap) { document.documentElement.classList.add('mo-ready'); console.warn('[motion] GSAP missing'); return; }
gsap.registerPlugin(ScrollTrigger);

const EASE = 'expo.out';                 // hard start, long settle
const D    = 1.1;

/* ---------------------------------------------------------
   0 · WAIT FOR FONTS
   Line breaks must be measured on the real typeface, not the
   fallback — otherwise SplitType produces the wrong lines.
   --------------------------------------------------------- */
try { await document.fonts.ready; } catch (e) {}
document.documentElement.classList.add('mo-ready');

/* ---------------------------------------------------------
   1 · SMOOTH SCROLL (Lenis)
   --------------------------------------------------------- */
let lenis = null;
if (window.Lenis && window.MO_SMOOTH !== false && !REDUCED && !TOUCH) {
  lenis = new Lenis({ lerp: .095, wheelMultiplier: 1, smoothWheel: true, syncTouch: false });
  lenis.on('scroll', ScrollTrigger.update);
  gsap.ticker.add(t => lenis.raf(t * 1000));
  gsap.ticker.lagSmoothing(0);

  // anchor links go through Lenis so easing stays consistent
  $$('a[href^="#"]').forEach(a => a.addEventListener('click', e => {
    const id = a.getAttribute('href');
    if (id.length < 2) return;
    const t = document.querySelector(id);
    if (!t) return;
    e.preventDefault();
    lenis.scrollTo(t, { offset: -80, duration: 1.4 });
  }));
}

addEventListener('scroll', () => ScrollTrigger.update(), { passive: true });

/* ---------------------------------------------------------
   2 · SPLIT HEADLINES INTO MASKED LINES
   --------------------------------------------------------- */
const HEADS = '.display, .final__h, .ask__q, .claim__val, .case__body h3, .ent__title';
let splits = [];

function buildLines() {
  splits.forEach(s => s.revert());
  splits = [];
  $$(HEADS).forEach(el => {
    const s = new SplitType(el, { types: 'lines', lineClass: 'ln' });
    $$('.ln', el).forEach(line => {
      const inner = document.createElement('span');
      inner.className = 'ln__i';
      while (line.firstChild) inner.appendChild(line.firstChild);
      line.appendChild(inner);
    });
    gsap.set(el, { opacity: 1 });
    splits.push(s);
  });
}
buildLines();

/* Reveal helper.
   Deliberately `set` + `to`, never `from`: ScrollTrigger.refresh()
   re-renders `from` tweens back to their start state, and once the
   trigger has fired-and-killed itself nothing brings the element
   back — that is how blocks ended up stuck at opacity 0. */
function reveal(targets, vars, trigger, opts = {}) {
  const t = gsap.utils.toArray(targets);
  if (!t.length) return;
  const start = Object.assign({ opacity: 0 }, vars.from || {});
  gsap.set(t, start);
  gsap.to(t, Object.assign({
    opacity: 1, x: 0, y: 0, yPercent: 0, scale: 1,
    duration: vars.duration || .95,
    ease: EASE,
    stagger: vars.stagger || 0,
    overwrite: 'auto',
    scrollTrigger: { trigger: trigger, start: opts.start || 'top 90%', once: true }
  }, vars.to || {}));
}

function revealHeading(el, trigger) {
  reveal($$('.ln__i', el), { from: { yPercent: 115, opacity: 1 }, duration: D, stagger: .085 }, trigger || el, { start: 'top 88%' });
}

/* ---------------------------------------------------------
   3 · HERO INTRO — one timeline, runs on load
   --------------------------------------------------------- */
const hero = $('.hero');
if (hero) {
  const tl = gsap.timeline({ defaults: { ease: EASE } });

  tl.from('.plate__labels .meta', { yPercent: 120, opacity: 0, duration: .9, stagger: .08 })
    .from($$('.plate__h .ln__i'), { yPercent: 115, duration: 1.3, stagger: .1 }, '-=.55')
    .from('.plate__mark', { opacity: 0, scale: .94, transformOrigin: 'right bottom', duration: 1.4 }, '-=1.1')
    .from('.plate__foot .lead', { y: 26, opacity: 0, duration: .9 }, '-=.95')
    .from('.hero__cta .btn', { y: 20, opacity: 0, duration: .8, stagger: .08 }, '-=.7')
    .from($$('.plate__claim .claim__val .ln__i'), { yPercent: 115, duration: 1.05, stagger: .06 }, '-=.8')
    .from('.plate__claim .claim__lab', { opacity: 0, duration: .7 }, '-=.6')
    .from('.plate__grid i', { scaleY: 0, transformOrigin: 'top center', duration: 1.2, stagger: .07 }, '-=1.5')
    .from('.band', { opacity: 0, duration: 1 }, '-=.4');
}

/* every other headline reveals on scroll */
$$(HEADS).forEach(el => { if (!hero || !hero.contains(el)) revealHeading(el); });

/* ---------------------------------------------------------
   4 · LEAD PARAGRAPHS + SMALL COPY
   --------------------------------------------------------- */
$$('.secdesc, .final__sub, .case__body p, .num__desc').forEach(p => {
  reveal(p, { from: { y: 24 } }, p, { start: 'top 90%' });
});

/* ---------------------------------------------------------
   5 · RULES ARE DRAWN, NOT SHOWN
   --------------------------------------------------------- */
function drawRules(root = document) {
  $$('.rule, .num, .case, .prov, .sechead, .acc__item, .bigfacts li, .ent__args li, .steps, .microfacts, .ftr__bottom', root)
    .forEach(n => {
      if (n.dataset.moDraw) return;
      const cs = getComputedStyle(n);
      const top = parseFloat(cs.borderTopWidth) >= .5;
      const bot = parseFloat(cs.borderBottomWidth) >= .5;
      if (!top && !bot) return;
      n.dataset.moDraw = '1';

      const line = document.createElement('i');
      line.style.cssText = `position:absolute;left:0;right:0;height:1px;background:${top ? cs.borderTopColor : cs.borderBottomColor};transform:scaleX(0);transform-origin:left center;pointer-events:none;` + (top ? 'top:-1px;' : 'bottom:-1px;');
      if (cs.position === 'static') n.style.position = 'relative';
      if (top) n.style.borderTopColor = 'transparent';
      else n.style.borderBottomColor = 'transparent';
      n.appendChild(line);

      gsap.to(line, {
        scaleX: 1, duration: 1.2, ease: EASE,
        scrollTrigger: { trigger: n, start: 'top 92%', once: true }
      });
    });
}
drawRules();

/* ---------------------------------------------------------
   6 · STAGGERED GROUPS
   --------------------------------------------------------- */
function stagGroup(sel, childSel) {
  $$(sel).forEach(box => {
    const kids = childSel ? $$(childSel, box) : [...box.children];
    if (!kids.length) return;
    reveal(kids, { from: { y: 30 }, stagger: .06 }, box, { start: 'top 88%' });
  });
}
['.filters', '.ask__opts', '.calc__opts', '.switch__pick', '.steps', '.bigfacts', '.ent__args',
 '.flow__targets', '.compare__list', '.compare__meta', '.intsteps', '.ftr__cols'].forEach(s => stagGroup(s));

/* ---------------------------------------------------------
   7 · SCHEMATIC — connectors draw on scrub, optionally pinned
   --------------------------------------------------------- */
$$('.netwrap').forEach(wrap => {
  const paths = $$('.net__links path', wrap);
  const nodes = $$('.node, .core', wrap);

  /* the connectors carry pathLength="100", so one dash figure fits
     every line — long and short ones finish drawing together instead
     of the lower ones lagging behind and reading as broken */
  paths.forEach(p => gsap.set(p, { strokeDasharray: 100, strokeDashoffset: 100 }));

  const band = wrap.closest('.band') || wrap;

  /* SVG groups: opacity only — scaling them needs an explicit
     svgOrigin and buys nothing here */
  reveal(nodes, { from: {}, duration: .7, stagger: .09 }, band, { start: 'top 85%' });

  /* keep the narrow-screen fallback label in sync with the ticker */
  const tick = $('#ticker'), tickM = $('#tickerMobile');
  if (tick && tickM) new MutationObserver(() => { tickM.textContent = tick.textContent; })
    .observe(tick, { childList: true, characterData: true, subtree: true });

  /* only the connectors are tied to scroll position — and they are all
     complete by the time the band is properly in view */
  gsap.to(paths, {
    strokeDashoffset: 0, ease: 'none', stagger: .05,
    scrollTrigger: {
      trigger: band,
      start: 'top 88%',
      end: 'top 55%',
      scrub: .7,
      invalidateOnRefresh: true
    }
  });
});

/* ---------------------------------------------------------
   8 · PARALLAX PLANES
   --------------------------------------------------------- */
$$('.vid, .term, .grow, .compare__col--after, .code').forEach(n => {
  gsap.fromTo(n, { y: 34 }, {
    y: -34, ease: 'none',
    scrollTrigger: { trigger: n, start: 'top bottom', end: 'bottom top', scrub: .8 }
  });
});

/* media surfaces open from the bottom edge */
$$('.tut__thumb, .band .netwrap').forEach(n => {
  gsap.set(n, { clipPath: 'inset(0 0 18% 0)' });
  gsap.to(n, {
    clipPath: 'inset(0 0 0% 0)', duration: 1.3, ease: EASE,
    scrollTrigger: { trigger: n, start: 'top 90%', once: true }
  });
});

/* ---------------------------------------------------------
   9 · BIG NUMBERS
   No horizontal drift: at 220px the numeral is wide enough
   that a 40px shift collided with its own label.
   --------------------------------------------------------- */
$$('.num__val').forEach(n => {
  gsap.set(n, { yPercent: 12, opacity: 0 });
  gsap.to(n, {
    yPercent: 0, opacity: 1, duration: 1.15, ease: EASE,
    scrollTrigger: { trigger: n.closest('.num'), start: 'top 85%', once: true }
  });
});

/* ---------------------------------------------------------
   10 · MARQUEE BAND  (model names, scroll-reactive)
   --------------------------------------------------------- */
if (!REDUCED) {
  const host = $('#value');
  if (host) {
    const names = ['Claude Opus 5','GPT-5.6 Sol','Gemini 3 Pro','Kimi K2.5','Mistral Large 3','Claude Sonnet 5','Gemini 3 Flash','GPT-5.6 Luna'];
    const band = document.createElement('div');
    band.className = 'mo-marquee';
    const row = document.createElement('div');
    row.className = 'mo-marquee__row';
    const chunk = names.map(n => `<span>${n}</span>`).join('');
    row.innerHTML = chunk + chunk;
    band.appendChild(row);
    host.insertAdjacentElement('afterend', band);

    const half = () => row.scrollWidth / 2;
    const loop = gsap.to(row, { x: () => -half(), duration: 26, ease: 'none', repeat: -1,
      modifiers: { x: gsap.utils.unitize(x => parseFloat(x) % half()) } });

    // scrolling speeds the belt up and flips its direction
    ScrollTrigger.create({
      onUpdate: self => {
        const v = gsap.utils.clamp(-6, 6, self.getVelocity() / 220);
        loop.timeScale(v === 0 ? 1 : (v < 0 ? -Math.abs(v) - 1 : v + 1));
        gsap.to(loop, { timeScale: 1, duration: 1.1, overwrite: true });
      }
    });
  }
}

/* ---------------------------------------------------------
   11 · (removed) drafting grid overlay
   The flashing guides were more noise than structure.
   Geometry now lives in the layout itself, not on top of it.
   --------------------------------------------------------- */

/* ---------------------------------------------------------
   12 · VERTICAL SECTION MARKERS
   --------------------------------------------------------- */
$$('.sec').forEach(sec => {
  const num = $('.secnum', sec);
  if (!num) return;
  const meta = $('.meta', sec);
  if (getComputedStyle(sec).position === 'static') sec.style.position = 'relative';

  const label = document.createElement('div');
  label.className = 'mo-vlabel';
  label.innerHTML = `<span><b>${num.textContent.trim()}</b> — ${(meta ? meta.textContent : '').trim()}</span>`;
  sec.appendChild(label);

  const tick = document.createElement('i');
  tick.className = 'mo-rot';
  num.prepend(tick);
  gsap.to(tick, {
    rotate: 180, ease: 'none',
    scrollTrigger: { trigger: sec, start: 'top bottom', end: 'bottom top', scrub: 1 }
  });
});

/* ---------------------------------------------------------
   13 · HEADER + PROGRESS RAIL
   --------------------------------------------------------- */
const hdr = $('#hdr');
if (hdr) ScrollTrigger.create({ start: 40, onEnter: () => hdr.classList.add('small'), onLeaveBack: () => hdr.classList.remove('small') });

if (!REDUCED) {
  const rail = document.createElement('div');
  rail.className = 'mo-rail';
  rail.innerHTML = '<i></i>';
  document.body.appendChild(rail);
  gsap.to(rail.firstChild, {
    scaleX: 1, ease: 'none',
    scrollTrigger: { start: 0, end: 'max', scrub: .3 }
  });
}

/* ---------------------------------------------------------
   14 · LABEL SWAP ON HOVER
   --------------------------------------------------------- */
$$('.btn, .nav a, .lnk--arrow').forEach(b => {
  if (b.querySelector('.sw') || b.children.length) return;
  const text = b.textContent.trim();
  if (!text) return;
  b.innerHTML = `<span class="sw"><i>${text}</i><i>${text}</i></span>`;
});

/* ---------------------------------------------------------
   16 · DYNAMIC CONTENT — app.js re-renders these
   --------------------------------------------------------- */
function decorate(host) {
  const fresh = $$('.prov, .pick, .tut', host).filter(n => !n.dataset.moSeen);
  if (!fresh.length) return;
  fresh.forEach(n => n.dataset.moSeen = '1');
  reveal(fresh, { from: { y: 34 }, duration: .9, stagger: .05 }, host, { start: 'top 85%' });
  drawRules(host);
  ScrollTrigger.refresh();
}
['#universe', '#picks', '#lib'].forEach(sel => {
  const host = $(sel);
  if (!host) return;
  decorate(host);
  new MutationObserver(() => decorate(host)).observe(host, { childList: true });
});

/* ---------------------------------------------------------
   17 · RESIZE — re-split lines, recompute triggers
   --------------------------------------------------------- */
let rz, lastW = innerWidth;
addEventListener('resize', () => {
  if (Math.abs(innerWidth - lastW) < 40) return;   // ignore mobile URL-bar jitter
  lastW = innerWidth;
  clearTimeout(rz);
  rz = setTimeout(() => { buildLines(); ScrollTrigger.refresh(); }, 250);
});

ScrollTrigger.refresh();
})();
