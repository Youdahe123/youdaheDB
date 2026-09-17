/* site.js — interaction shared by every page: a reading-progress bar, a ⌘K
   command palette built from the page's own headings, and the shortcut help.
   Loaded after the theme script, so it can assume a theme is already applied. */
(function () {
'use strict';

/* ── reading progress ─────────────────────────────────────── */
function progress() {
  var bar = document.createElement('div');
  bar.className = 'readbar';
  bar.innerHTML = '<i></i>';
  document.body.appendChild(bar);
  var fill = bar.firstChild, pct = document.querySelector('.readpct');
  var ticking = false;

  function upd() {
    var h = document.documentElement;
    var max = h.scrollHeight - innerHeight;
    var p = max > 0 ? Math.min(1, Math.max(0, h.scrollTop / max)) : 0;
    fill.style.transform = 'scaleX(' + p + ')';
    if (pct) pct.textContent = Math.round(p * 100) + '% read';
    ticking = false;
  }
  addEventListener('scroll', function () {
    if (!ticking) { ticking = true; requestAnimationFrame(upd); }
  }, { passive: true });
  addEventListener('resize', upd);
  upd();
}

/* ── command palette ──────────────────────────────────────── */
function palette() {
  var PAGES = [
    { label:'Overview',     hint:'index.html',   href:'index.html' },
    { label:'Get started',  hint:'start.html',   href:'start.html' },
    { label:'Docs',         hint:'docs.html',    href:'docs.html' },
    { label:'Console',      hint:'console.html', href:'console.html' },
    { label:'GitHub · repo', hint:'external',    href:'https://github.com/Youdahe123/rust-db' }
  ];

  var items = PAGES.slice();
  /* Every h2 becomes a jump target. Matching a heading alone is too strict —
     "vector" should find "Chunk, embed, score, rank" — so each entry also
     carries its section's kicker and lede as hidden search text. */
  document.querySelectorAll('main h2').forEach(function (h, i) {
    if (!h.id) h.id = 'h-' + i;
    var sec = h.closest('section') || h.parentElement;
    var kicker = sec.querySelector('.kicker');
    var lede = sec.querySelector('.lede');
    items.push({
      label: h.textContent.trim(),
      hint: kicker ? kicker.textContent.trim().toLowerCase() : 'on this page',
      keys: (lede ? lede.textContent : '').slice(0, 260),
      href: '#' + h.id, local: true
    });
  });
  items.push({ label:'Toggle light / dark', hint:'theme', action:function () {
    var b = document.getElementById('theme-toggle'); if (b) b.click();
  }});

  var el = document.createElement('div');
  el.className = 'pal';
  el.innerHTML =
    '<div class="pal-box" role="dialog" aria-modal="true" aria-label="Command palette">' +
      '<input id="pal-in" placeholder="Jump to a page or a section…" autocomplete="off" spellcheck="false">' +
      '<div class="pal-list" id="pal-list"></div>' +
      '<div class="pal-foot"><span><kbd>↑</kbd><kbd>↓</kbd> move</span>' +
      '<span><kbd>↵</kbd> open</span><span><kbd>esc</kbd> close</span></div>' +
    '</div>';
  document.body.appendChild(el);

  var input = el.querySelector('#pal-in'), list = el.querySelector('#pal-list');
  var shown = [], sel = 0, open = false;

  /* Fuzzy subsequence on the title only — that is what makes "gst" find "Get
     started". Body text is matched as a plain substring instead, because a
     subsequence over a couple of hundred characters matches nearly anything.
     Lower rank = better; title hits always sort above body hits. */
  function sub(q, s) {
    var i = 0;
    for (var j = 0; j < s.length && i < q.length; j++) if (s[j] === q[i]) i++;
    return i === q.length;
  }
  function rank(q, it) {
    if (!q) return 0;
    q = q.toLowerCase();
    var label = it.label.toLowerCase(), hint = (it.hint || '').toLowerCase();
    if (label.indexOf(q) === 0) return 0;          // prefix
    if (label.indexOf(q) > -1) return 1;           // substring
    if (sub(q, label)) return 2;                   // initials / skipped letters
    if (hint.indexOf(q) > -1) return 3;
    if (q.length >= 3 && (it.keys || '').toLowerCase().indexOf(q) > -1) return 4;
    return -1;
  }

  function render() {
    var q = input.value.trim();
    shown = items.map(function (it) { return { it: it, r: rank(q, it) }; })
                 .filter(function (x) { return x.r > -1; })
                 .sort(function (a, b) { return a.r - b.r; })
                 .map(function (x) { return x.it; });
    if (sel >= shown.length) sel = Math.max(0, shown.length - 1);
    list.innerHTML = shown.length ? shown.map(function (it, i) {
      return '<div class="pal-row' + (i === sel ? ' on' : '') + '" data-i="' + i + '">' +
             '<span>' + it.label + '</span><span class="pal-hint">' + it.hint + '</span></div>';
    }).join('') : '<div class="pal-empty">No match.</div>';
    list.querySelectorAll('.pal-row').forEach(function (r) {
      r.onmouseenter = function () { sel = +r.dataset.i; render(); };
      r.onclick = function () { go(shown[+r.dataset.i]); };
    });
  }

  function go(it) {
    if (!it) return;
    close();
    if (it.action) return it.action();
    if (it.local) {
      var t = document.querySelector(it.href);
      if (t) t.scrollIntoView({ behavior: 'smooth', block: 'start' });
      return;
    }
    location.href = it.href;
  }

  function show() {
    open = true; el.classList.add('on'); input.value = ''; sel = 0; render();
    input.focus();
  }
  function close() { open = false; el.classList.remove('on'); }

  el.addEventListener('mousedown', function (e) { if (e.target === el) close(); });
  input.addEventListener('input', function () { sel = 0; render(); });

  addEventListener('keydown', function (e) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault(); open ? close() : show(); return;
    }
    if (!open) {
      var tag = (e.target.tagName || '').toLowerCase();
      if (e.key === '/' && tag !== 'input' && tag !== 'textarea') { e.preventDefault(); show(); }
      return;
    }
    if (e.key === 'Escape') { e.preventDefault(); close(); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); sel = Math.min(shown.length - 1, sel + 1); render(); }
    else if (e.key === 'ArrowUp')   { e.preventDefault(); sel = Math.max(0, sel - 1); render(); }
    else if (e.key === 'Enter')     { e.preventDefault(); go(shown[sel]); }
  });

  document.querySelectorAll('[data-palette]').forEach(function (b) { b.onclick = show; });
}

/* ── reveal on scroll ─────────────────────────────────────
   Deliberately conservative: content must never be left invisible because an
   observer did not fire. Anything already on screen is shown immediately with
   no transition, and a failsafe reveals everything regardless after 2s. */
function reveal() {
  var targets = [].slice.call(document.querySelectorAll('main > .sec, main > .serving'));
  if (!targets.length) return;

  function showAll() { targets.forEach(function (n) { n.classList.add('in'); }); }
  if (!window.IntersectionObserver || matchMedia('(prefers-reduced-motion: reduce)').matches) {
    return showAll();
  }

  targets.forEach(function (n) {
    /* already in view on load — no animation, it would just look like a flicker */
    var r = n.getBoundingClientRect();
    if (r.top < innerHeight && r.bottom > 0) { n.classList.add('in'); return; }
    n.classList.add('rv');
  });

  var io = new IntersectionObserver(function (entries) {
    entries.forEach(function (en) {
      if (en.isIntersecting) { en.target.classList.add('in'); io.unobserve(en.target); }
    });
  }, { rootMargin: '0px 0px -6% 0px', threshold: 0.04 });
  targets.forEach(function (n) { if (n.classList.contains('rv')) io.observe(n); });

  setTimeout(showAll, 2000);   // failsafe: never strand content at opacity 0
}

function boot() { progress(); palette(); reveal(); }
if (document.readyState === 'loading') addEventListener('DOMContentLoaded', boot);
else boot();

})();
