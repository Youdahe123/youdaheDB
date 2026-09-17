/* intro.js — the opening sequence.
 *
 * The overlay is built in JS and never exists in the HTML, so if this file
 * fails to load or throws, the page is simply visible with no intro. Every
 * hidden state is applied by script and has a failsafe timer that clears it
 * unconditionally — content must never be strandable behind an animation.
 *
 * Runs once per tab session, and not at all under prefers-reduced-motion.
 */
(function () {
  'use strict';

  var KEY = 'ydb-intro';
  var MARK = '<svg viewBox="0 0 24 24" aria-hidden="true">' +
    '<path class="s s1" d="M12 2.8 21 5.4 12 8 3 5.4Z"/>' +
    '<path class="s s2" d="M12 9.4 21 12 12 14.6 3 12Z"/>' +
    '<path class="s s3" d="M12 16 21 18.6 12 21.2 3 18.6Z"/></svg>';

  /* wrap each word in a mask so it can slide up independently */
  function splitWords(root) {
    var walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, null);
    var nodes = [], n;
    while ((n = walker.nextNode())) nodes.push(n);
    var out = [];
    nodes.forEach(function (node) {
      if (!node.nodeValue.trim()) return;
      var frag = document.createDocumentFragment();
      node.nodeValue.split(/(\s+)/).forEach(function (tok) {
        if (!tok.trim()) { frag.appendChild(document.createTextNode(tok)); return; }
        var s = document.createElement('span'); s.className = 'w';
        var i = document.createElement('i'); i.textContent = tok;
        s.appendChild(i); frag.appendChild(s); out.push(i);
      });
      node.parentNode.replaceChild(frag, node);
    });
    return out;
  }

  function revealHero(animate) {
    var h = document.querySelector('.hero h1, .mhead h1, .article h1, .doc h1, .gtop h1');
    if (!h) return;
    if (!animate) return;                       // leave the DOM untouched
    var words = splitWords(h);
    if (!words.length) return;
    h.classList.add('hero-anim');
    var step = document.querySelector('.intro') ? 42 : 28;   // brisker when there is no overlay
    words.forEach(function (w, i) { w.style.transitionDelay = (i * step) + 'ms'; });
    requestAnimationFrame(function () {
      requestAnimationFrame(function () { h.classList.add('hero-in'); });
    });
    /* failsafe: whatever happens above, the words become visible */
    setTimeout(function () { h.classList.add('hero-in'); }, 2600);
  }

  var reduce = false;
  try { reduce = matchMedia('(prefers-reduced-motion: reduce)').matches; } catch (e) {}
  var seen = false;
  try { seen = sessionStorage.getItem(KEY) === '1'; } catch (e) {}

  /* Three cases:
       reduced motion  — nothing moves at all
       already visited — headline stagger only, so each page still "arrives"
       first visit     — the full slab-and-counter overlay, then the stagger
     The overlay is deliberately once per session: a 1.2s hold is a nice
     entrance and an infuriating page transition. */
  if (reduce) return;
  if (seen) { revealHero(true); return; }
  try { sessionStorage.setItem(KEY, '1'); } catch (e) {}

  var el = document.createElement('div');
  el.className = 'intro';
  el.setAttribute('aria-hidden', 'true');
  el.innerHTML = '<div class="intro-in"><div class="intro-mark">' + MARK +
                 '</div><div class="intro-n">000</div></div>';
  document.body.appendChild(el);
  document.documentElement.classList.add('intro-lock');

  var nEl = el.querySelector('.intro-n');
  var started = performance.now(), DUR = 900;

  function tick(now) {
    var p = Math.min(1, (now - started) / DUR);
    var eased = 1 - Math.pow(1 - p, 3);
    nEl.textContent = String(Math.round(eased * 100)).padStart(3, '0');
    if (p < 1) requestAnimationFrame(tick);
  }
  requestAnimationFrame(tick);

  var done = false;
  function finish() {
    if (done) return; done = true;
    el.classList.add('out');
    document.documentElement.classList.remove('intro-lock');
    revealHero(true);
    setTimeout(function () { if (el.parentNode) el.parentNode.removeChild(el); }, 900);
  }
  setTimeout(finish, 1180);
  setTimeout(function () {                       // failsafe: never trap the page
    document.documentElement.classList.remove('intro-lock');
    if (el.parentNode) el.parentNode.removeChild(el);
  }, 4000);

  /* let an impatient visitor skip it */
  el.addEventListener('click', finish);
  addEventListener('keydown', function (e) { if (e.key === 'Escape') finish(); });
})();
