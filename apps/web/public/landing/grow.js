/* apiToken landing — shared, clickable catalog cloud.
   app.js/app-en.js own ALL_MODELS and navigation to each catalog card.
   CSS provides the visible static layout; only a roomy mouse-driven field floats. */
(() => {
  const field = document.getElementById('growField');
  if (!field || typeof ALL_MODELS === 'undefined') return;

  const NARROW = matchMedia('(max-width:820px)');
  const STILL = matchMedia('(prefers-reduced-motion: reduce)');
  const FINE = matchMedia('(hover: hover) and (pointer: fine)');
  const GAP = 10, RADIUS = 110, MAX_SPEED = 40;
  let fw = 0, fh = 0, raf = 0, last = 0, ready = false, visible = false;
  let floating = false, hovered = false, pressed = false, observedW = -1, observedH = -1;
  const pointer = { active: false, x: 0, y: 0 };

  const balls = ALL_MODELS.map(model => {
    const link = document.createElement('a');
    link.className = 'grow__chip' + (model.isNew ? ' new' : '');
    link.href = '#model-' + model.id;
    link.dataset.modelId = model.id;
    link.textContent = model.name;
    field.appendChild(link);
    const angle = Math.random() * Math.PI * 2, speed = 14 + Math.random() * 8;
    return { el: link, x: 0, y: 0, w: 0, h: 0, speed,
      vx: Math.cos(angle) * speed, vy: Math.sin(angle) * speed };
  });

  const eligible = () => !NARROW.matches && !STILL.matches && FINE.matches;
  const stop = () => { cancelAnimationFrame(raf); raf = 0; last = 0; };
  const canRun = () => floating && visible && eligible() && !hovered && !pressed &&
    !field.contains(document.activeElement) && !document.hidden;
  const run = () => { if (!raf && canRun()) raf = requestAnimationFrame(tick); };
  const paint = () => balls.forEach(b => { b.el.style.translate = `${b.x}px ${b.y}px`; });
  const rememberSize = () => { observedW = field.clientWidth; observedH = field.clientHeight; };
  const makeStatic = () => {
    stop();
    floating = false;
    field.classList.remove('is-floating');
    balls.forEach(b => b.el.style.removeProperty('translate'));
    rememberSize();
  };

  // Fit complete rows first. If they do not fit, leave every link in CSS flex flow.
  // Extra space spreads rows and chips out without random overlap or edge clamping.
  const fit = () => {
    const width = fw - GAP * 2, height = fh - GAP * 2, rows = [];
    let row = null;
    for (const b of balls) {
      b.w = b.el.offsetWidth; b.h = b.el.offsetHeight;
      if (!b.w || !b.h || b.w > width || b.h > height) return false;
      if (!row || row.w + GAP + b.w > width) {
        row = { items: [], w: 0, h: 0 };
        rows.push(row);
      }
      row.w += (row.items.length ? GAP : 0) + b.w;
      row.h = Math.max(row.h, b.h);
      row.items.push(b);
    }
    const used = rows.reduce((sum, r) => sum + r.h, 0) + Math.max(0, rows.length - 1) * GAP;
    if (!rows.length || used > height) return false;
    const extraY = (height - used) / (rows.length + 1);
    let y = GAP + extraY;
    rows.forEach(r => {
      const extraX = (width - r.w) / (r.items.length + 1);
      let x = GAP + extraX;
      r.items.forEach(b => {
        b.x = x; b.y = y + (r.h - b.h) / 2;
        x += b.w + GAP + extraX;
      });
      y += r.h + GAP + extraY;
    });
    return true;
  };

  const layout = () => {
    stop();
    if (!ready || !eligible()) { makeStatic(); return; }
    field.classList.add('is-floating');
    fw = field.clientWidth; fh = field.clientHeight;
    if (!fit()) { makeStatic(); return; }
    floating = true;
    paint(); rememberSize(); run();
  };

  const blocked = (b, x, y) => balls.some(other => other !== b &&
    x < other.x + other.w + GAP - .001 && x + b.w + GAP > other.x + .001 &&
    y < other.y + other.h + GAP - .001 && y + b.h + GAP > other.y + .001);

  function tick(now) {
    raf = 0;
    // A media-query change must also stop a frame that was already queued.
    if (!eligible()) { makeStatic(); return; }
    if (!canRun()) { last = 0; return; }
    const dt = last ? Math.min((now - last) / 1000, .05) : 0;
    last = now;
    for (const b of balls) {
      if (pointer.active) {
        const dx = b.x + b.w / 2 - pointer.x, dy = b.y + b.h / 2 - pointer.y;
        const distance = Math.hypot(dx, dy);
        if (distance > 0 && distance < RADIUS) {
          const force = (1 - distance / RADIUS) * 80 * dt;
          b.vx += dx / distance * force; b.vy += dy / distance * force;
        }
      }
      // The brief mouse impulse settles back to the original slow drift.
      const speed = Math.hypot(b.vx, b.vy);
      if (speed) {
        const nextSpeed = Math.min(MAX_SPEED, speed + (b.speed - speed) * Math.min(1, dt * 1.5));
        b.vx *= nextSpeed / speed; b.vy *= nextSpeed / speed;
      }

      // Admit one axis at a time. A rejected move bounces, so a collision can
      // never push a neighbour through another chip or outside the field.
      let x = b.x + b.vx * dt;
      if (x < GAP || x + b.w > fw - GAP) {
        b.vx = -b.vx; x = Math.max(GAP, Math.min(x, fw - GAP - b.w));
      }
      if (blocked(b, x, b.y)) b.vx = -b.vx; else b.x = x;
      let y = b.y + b.vy * dt;
      if (y < GAP || y + b.h > fh - GAP) {
        b.vy = -b.vy; y = Math.max(GAP, Math.min(y, fh - GAP - b.h));
      }
      if (blocked(b, b.x, y)) b.vy = -b.vy; else b.y = y;
    }
    paint(); run();
  }

  // A link is always catchable: hovering, pressing or focusing one freezes all chips.
  const trackPointer = e => {
    const rect = field.getBoundingClientRect();
    pointer.x = e.clientX - rect.left - field.clientLeft;
    pointer.y = e.clientY - rect.top - field.clientTop;
    pointer.active = e.pointerType === 'mouse';
    hovered = !!e.target.closest('.grow__chip');
    if (hovered) stop(); else run();
  };
  field.addEventListener('pointerover', trackPointer);
  field.addEventListener('pointermove', trackPointer);
  field.addEventListener('pointerleave', () => {
    pointer.active = false; hovered = false; run();
  });
  field.addEventListener('pointerdown', () => { pressed = true; stop(); });
  const release = () => { pressed = false; run(); };
  window.addEventListener('pointerup', release);
  window.addEventListener('pointercancel', release);
  field.addEventListener('focusin', stop);
  field.addEventListener('focusout', () => queueMicrotask(run));

  [NARROW, STILL, FINE].forEach(query => query.addEventListener('change', layout));
  document.addEventListener('visibilitychange', () => { if (document.hidden) stop(); else run(); });
  new IntersectionObserver(entries => {
    visible = entries[0].isIntersecting;
    if (visible) run(); else stop();
  }, { threshold: 0 }).observe(field);
  new ResizeObserver(() => {
    if (ready && (field.clientWidth !== observedW || field.clientHeight !== observedH)) layout();
  }).observe(field);

  Promise.resolve(document.fonts?.ready).catch(() => {}).then(() => { ready = true; layout(); });
})();
