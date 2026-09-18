/* tracer.js — the canvas trace engine behind the diagrams on home.html.
   One instance per diagram, driven by a plain {nodes, edges, ops} description.
   Every colour is read from a CSS custom property, so a diagram follows the
   theme for free. No build step; the page includes this with a <script> tag.

   A step is {from, to, t, d, dur} where `to` may be an array — that fans a
   packet out to several nodes at once, which is how the batched read and the
   replica hedge are drawn. Flags: hit (terminal success), skip (work avoided),
   hold (a blocking wait, drawn in place), slow (a replica that lags). */
(function (global) {
  'use strict';

  var CSSV = (function () {
    var cache = {};
    addEventListener('themechange', function () { cache = {}; });
    return function (n) {
      if (cache[n] === undefined) {
        cache[n] = getComputedStyle(document.documentElement).getPropertyValue(n).trim();
      }
      return cache[n];
    };
  })();

  var SPEED = 0.55;                 // global tempo multiplier for every trace
  var ease = function (t) { return t < .5 ? 2*t*t : 1 - Math.pow(-2*t+2, 2)/2; };

  function createTracer(cfg) {
    var canvas = cfg.canvas, nodes = cfg.nodes, edges = cfg.edges, ops = cfg.ops;
    var stepsEl = cfg.stepsEl, footEl = cfg.footEl, hintEl = cfg.hintEl, tipEl = cfg.tipEl;
    var buttons = Array.prototype.slice.call(cfg.buttons);
    var idle = cfg.idleHint || 'click an operation to trace it through the engine';
    var ctx = canvas.getContext('2d');

    var W = 0, H = 0, dpr = Math.min(devicePixelRatio || 1, 2);
    var active = null, visited = {}, pulsing = {}, running = false, hover = null;

    function size() {
      var b = canvas.parentElement.getBoundingClientRect();
      W = b.width; H = b.height;
      canvas.width = W * dpr; canvas.height = H * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }
    function px(n) { return { x: nodes[n].x * W, y: nodes[n].y * H }; }
    function kindCol(k) {
      return CSSV(k === 'read' ? '--cat-3' : k === 'bg' ? '--cat-2' : '--cat-1');
    }
    function targets(s) { return Array.isArray(s.to) ? s.to : [s.to]; }
    function isEdge(a, b) {
      for (var i = 0; i < edges.length; i++) {
        if ((edges[i][0] === a && edges[i][1] === b) || (edges[i][0] === b && edges[i][1] === a)) return true;
      }
      return false;
    }

    function draw(ts) {
      ctx.clearRect(0, 0, W, H);
      var col = active ? kindCol(active.op.kind) : CSSV('--cat-1');
      var cur = active && active.cur;

      /* edges — the ones carrying the current hop light up */
      for (var i = 0; i < edges.length; i++) {
        var a = edges[i][0], b = edges[i][1], p = px(a), q = px(b), live = false;
        if (cur) {
          var tg = targets(cur);
          for (var j = 0; j < tg.length; j++) {
            if ((cur.from === a && tg[j] === b) || (cur.from === b && tg[j] === a)) live = true;
          }
        }
        ctx.strokeStyle = live ? col : CSSV('--cv-edge');
        ctx.lineWidth = live ? 1.6 : 1;
        ctx.beginPath(); ctx.moveTo(p.x, p.y); ctx.lineTo(q.x, q.y); ctx.stroke();
      }

      /* a hop with no drawn edge (a return, a causal step) — dash it in */
      if (cur) {
        var tgs = targets(cur);
        for (var k = 0; k < tgs.length; k++) {
          if (tgs[k] === cur.from || isEdge(cur.from, tgs[k])) continue;
          var pp = px(cur.from), qq = px(tgs[k]);
          ctx.save(); ctx.setLineDash([4, 5]);
          ctx.strokeStyle = col; ctx.globalAlpha = .5; ctx.lineWidth = 1.3;
          ctx.beginPath(); ctx.moveTo(pp.x, pp.y); ctx.lineTo(qq.x, qq.y); ctx.stroke();
          ctx.restore();
        }
      }

      /* pulse rings, drawn under the labels so they never wash one out */
      for (var id in nodes) {
        var pu = pulsing[id]; if (pu === undefined) continue;
        var g = Math.max(0, 1 - (ts - pu) / 900); if (g <= .05) continue;
        var c = px(id);
        ctx.strokeStyle = col; ctx.globalAlpha = g * .5; ctx.lineWidth = 1.4;
        ctx.beginPath(); ctx.arc(c.x, c.y, nodes[id].r + 8 + (1 - g) * 16, 0, 7); ctx.stroke();
        ctx.globalAlpha = 1;
      }

      /* nodes */
      for (var nid in nodes) {
        var n = nodes[nid], c2 = px(nid), seen = !!visited[nid], glow = 0;
        var pl = pulsing[nid];
        if (pl !== undefined) {
          glow = Math.max(0, 1 - (ts - pl) / 900);
          if (glow <= 0) delete pulsing[nid];
        }
        var base = n.lv !== undefined ? CSSV('--lv-' + n.lv) : CSSV('--cv-node');
        var isHover = hover === nid;

        if (glow > 0) { ctx.shadowColor = col; ctx.shadowBlur = 6 + glow * 26; }
        ctx.fillStyle = glow > 0 ? col : (seen ? CSSV('--cv-node-seen') : base);
        ctx.beginPath(); ctx.arc(c2.x, c2.y, n.r + glow * 3.5 + (isHover ? 2 : 0), 0, 7); ctx.fill();
        ctx.shadowBlur = 0;
        ctx.strokeStyle = isHover ? col : CSSV('--cv-ring');
        ctx.lineWidth = isHover ? 2.4 : 2; ctx.stroke();

        ctx.textAlign = 'center';
        ctx.font = '500 11.5px ui-monospace, SFMono-Regular, Menlo, monospace';
        ctx.fillStyle = glow > 0 ? CSSV('--cv-on-glow')
                      : (seen || isHover ? CSSV('--cv-text') : CSSV('--cv-text-dim'));
        ctx.fillText(n.label, c2.x, c2.y - n.r - 10);
        if (n.sub) {
          ctx.font = '400 9.5px ui-monospace, SFMono-Regular, Menlo, monospace';
          ctx.fillStyle = CSSV('--cv-sub');
          ctx.fillText(n.sub, c2.x, c2.y + n.r + 15);
        }
      }

      /* the travelling packets — one per target, so a fan-out reads as a fan-out */
      if (cur && active.startedAt) {
        var tt = targets(cur);
        for (var m = 0; m < tt.length; m++) {
          if (tt[m] === cur.from) continue;
          var s = px(cur.from), e2 = px(tt[m]);
          var raw = Math.min(1, (ts - active.startedAt) / (cur.dur * SPEED));
          /* a lagging replica arrives late — that is the whole point of a hedge */
          if (cur.slow && m === 0) raw = Math.min(1, raw * 0.45);
          var t = ease(raw);
          for (var tr = 1; tr <= 5; tr++) {
            var te = Math.max(0, t - tr * 0.035);
            ctx.globalAlpha = (1 - tr / 5) * .35; ctx.fillStyle = col;
            ctx.beginPath();
            ctx.arc(s.x + (e2.x - s.x) * te, s.y + (e2.y - s.y) * te, 4.5 - tr * .6, 0, 7);
            ctx.fill();
          }
          ctx.globalAlpha = 1;
          ctx.shadowColor = col; ctx.shadowBlur = 16; ctx.fillStyle = col;
          ctx.beginPath();
          ctx.arc(s.x + (e2.x - s.x) * t, s.y + (e2.y - s.y) * t, 5.5, 0, 7); ctx.fill();
          ctx.shadowBlur = 0;
        }
      }
      requestAnimationFrame(draw);
    }

    function renderSteps(op, upto) {
      stepsEl.innerHTML = op.steps.map(function (s, i) {
        return '<div class="tr-step ' + (i <= upto ? 'on' : '') + ' ' + (s.hit ? 'hit' : '') +
               ' ' + (s.skip ? 'skip' : '') + '">' +
               '<span class="i">' + String(i + 1).padStart(2, '0') + '</span>' +
               '<span class="b"><span class="t">' + s.t + '</span>' +
               '<span class="d">' + s.d + '</span></span></div>';
      }).join('');
    }

    function wait(ms) { return new Promise(function (r) { setTimeout(r, ms); }); }

    async function run(key) {
      if (running || !ops[key]) return;
      running = true;
      buttons.forEach(function (b) {
        b.disabled = true; b.classList.toggle('on', b.dataset.op === key);
      });
      var op = ops[key];
      active = { op: op, cur: null, startedAt: 0 };
      visited = {}; pulsing = {};
      footEl.textContent = '';
      if (hintEl) hintEl.textContent = 'running · ' + op.title;
      renderSteps(op, -1);

      for (var i = 0; i < op.steps.length; i++) {
        var s = op.steps[i];
        active.cur = s; active.startedAt = performance.now();
        renderSteps(op, i);
        visited[s.from] = true; pulsing[s.from] = performance.now();
        await wait(s.dur * SPEED);
        targets(s).forEach(function (t) { visited[t] = true; pulsing[t] = performance.now(); });
        active.cur = null;
        await wait(70);
      }

      footEl.textContent = op.foot;
      if (hintEl) hintEl.textContent = idle;
      active = null; running = false;
      buttons.forEach(function (b) { b.disabled = false; });
    }

    /* hover a node to find out what it is — the diagram should be explorable
       when it is sitting still, not only while a trace is running */
    canvas.addEventListener('mousemove', function (ev) {
      var r = canvas.getBoundingClientRect();
      var mx = ev.clientX - r.left, my = ev.clientY - r.top, found = null;
      for (var id in nodes) {
        var c = px(id), dx = mx - c.x, dy = my - c.y;
        if (dx * dx + dy * dy <= Math.pow(nodes[id].r + 9, 2)) { found = id; break; }
      }
      hover = found;
      canvas.style.cursor = found ? 'help' : 'default';
      if (!tipEl) return;
      if (found && nodes[found].tip) {
        tipEl.textContent = nodes[found].tip;
        tipEl.style.left = (ev.clientX + 14) + 'px';
        tipEl.style.top = (ev.clientY + 14) + 'px';
        tipEl.classList.add('on');
      } else { tipEl.classList.remove('on'); }
    });
    canvas.addEventListener('mouseleave', function () {
      hover = null; if (tipEl) tipEl.classList.remove('on');
    });

    buttons.forEach(function (b) { b.onclick = function () { run(b.dataset.op); }; });
    addEventListener('resize', size);
    if (global.ResizeObserver) new ResizeObserver(size).observe(canvas.parentElement);
    size(); requestAnimationFrame(draw);

    return { run: run, keys: Object.keys(ops), isRunning: function () { return running; } };
  }

  global.createTracer = createTracer;
  global.CSSV = CSSV;
})(window);
