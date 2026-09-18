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

/* ── announcement bar ─────────────────────────────────────────
   One rotating strip above the nav, on every page that has room for it.
   Sits in normal flow rather than sticky, so it scrolls away and leaves the
   nav to do the sticking. */
function announce() {
  var page = (location.pathname.split('/').pop() || 'index').replace(/\.html$/, '');
  /* console locks the viewport (body overflow:hidden over a 100vh shell), so an
     extra row of chrome pushes its bottom edge off screen. access IS the waitlist. */
  if (page === 'console' || page === 'access') return;
  if (document.querySelector('.shell')) return;
  try { if (localStorage.getItem('ydb-ann') === 'off') return; } catch (e) {}

  var nav = document.querySelector('.topnav');
  if (!nav || !nav.parentNode) return;

  var MSGS = [
    '<span class="tag">Beta</span><span><b>Looking for beta users.</b> The storage engine runs today — early access as each layer lands.</span>',
    '<span class="tag">Waitlist</span><span><b>Early access goes out in batches.</b> A sandbox passcode, plus build notes as they ship.</span>',
    '<span class="tag">Status</span><span>Built in the open, one layer at a time — <b>v0.1, active development.</b></span>'
  ];

  var bar = document.createElement('div');
  bar.className = 'ann';
  bar.innerHTML =
    '<div class="in">' +
      '<i class="ann-dot"></i>' +
      '<div class="ann-rot">' +
        MSGS.map(function (m, i) {
          return '<div class="ann-msg' + (i ? '' : ' on') + '">' + m + '</div>';
        }).join('') +
      '</div>' +
      '<div class="ann-cta">Join the waitlist &rarr;</div>' +
      '<form class="ann-form" novalidate>' +
        '<span class="ann-lead">Beta access</span>' +
        '<input type="email" placeholder="you@company.com" autocomplete="email" aria-label="Email address">' +
        '<input class="ann-hp" type="text" tabindex="-1" autocomplete="off" aria-hidden="true">' +
        '<button type="submit">Join</button>' +
        '<span class="ann-note"></span>' +
      '</form>' +
      '<div class="ann-x" role="button" tabindex="0" aria-label="Dismiss">&times;</div>' +
    '</div>';
  nav.parentNode.insertBefore(bar, nav);

  /* ── rotation ── */
  var msgs = bar.querySelectorAll('.ann-msg'), at = 0, timer = null, paused = false;
  var still = matchMedia('(prefers-reduced-motion: reduce)').matches;

  function step() {
    if (paused || document.hidden) return;
    msgs[at].classList.remove('on');
    at = (at + 1) % msgs.length;
    msgs[at].classList.add('on');
  }
  /* Reduced motion still rotates — the transition is what gets dropped, in CSS.
     Stopping entirely would hide two of the three messages permanently. */
  if (msgs.length > 1) timer = setInterval(step, still ? 7000 : 5200);
  bar.addEventListener('mouseenter', function () { paused = true; });
  bar.addEventListener('mouseleave', function () { paused = false; });

  /* ── dismiss ── */
  function dismiss() {
    if (timer) clearInterval(timer);
    bar.remove();
    try { localStorage.setItem('ydb-ann', 'off'); } catch (e) {}
  }
  var x = bar.querySelector('.ann-x');
  x.addEventListener('click', dismiss);
  x.addEventListener('keydown', function (e) {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); dismiss(); }
  });

  /* ── signup ── */
  var form = bar.querySelector('.ann-form');
  var input = form.querySelector('input[type=email]');
  var hp = form.querySelector('.ann-hp');
  var btn = form.querySelector('button');
  var note = form.querySelector('.ann-note');

  bar.querySelector('.ann-cta').addEventListener('click', function () {
    bar.classList.add('open');
    if (timer) { clearInterval(timer); timer = null; }
    input.focus();
  });

  /* waitlist.js and its config only ship on two pages, so pull them in on first
     use rather than adding two script tags to all fourteen. */
  function waitlist(done) {
    if (window.YDBWaitlist) return done(true);
    var load = function (src, next) {
      var s = document.createElement('script');
      s.src = src;
      s.onload = function () { next(); };
      s.onerror = function () { next(); };
      document.head.appendChild(s);
    };
    load('config.js', function () {
      load('waitlist.js', function () { done(!!window.YDBWaitlist); });
    });
  }

  form.addEventListener('submit', function (e) {
    e.preventDefault();
    if (hp.value) { note.className = 'ann-note ok'; note.textContent = "You're on the list."; return; }

    btn.disabled = true;
    note.className = 'ann-note';
    note.textContent = 'Sending…';

    waitlist(function (ok) {
      if (!ok) { btn.disabled = false; location.href = 'access.html'; return; }

      YDBWaitlist.join(input.value, 'announce-bar-' + page).then(function (r) {
        btn.disabled = false;
        if (r.status === 'ok') {
          note.className = 'ann-note ok';
          note.textContent = "You're on the list — we'll email when a slot opens.";
          input.disabled = true; btn.disabled = true;
        } else if (r.status === 'invalid') {
          note.textContent = 'That address does not look right.';
          input.select();
        } else if (r.status === 'network') {
          note.textContent = "Couldn't reach the waitlist. Try again?";
        } else if (r.status === 'unconfigured') {
          location.href = 'access.html';
        } else {
          note.textContent = 'Something went wrong. Try again?';
          console.error('[waitlist]', r.code, r.detail);
        }
      });
    });
  });
}

function boot() { announce(); progress(); palette(); reveal(); }
if (document.readyState === 'loading') addEventListener('DOMContentLoaded', boot);
else boot();

})();
