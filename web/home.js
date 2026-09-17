/* home.js — everything interactive on the overview page.
   Two trace diagrams (the storage engine that exists, the serving path that is
   still a design), a retrieval playground that actually computes, and a
   workload model. Nothing here reports a measured number: the engine diagram
   states structural facts (how many fsyncs, how many files), and the model
   does arithmetic on whatever you type into it. */
(function () {
'use strict';

/* ══════════════════════════════════════════════════════════════
   1. The storage engine — this part is built.
   Nodes are hand-placed in normalised coords so the layout is
   deliberate rather than whatever a force sim settles into.
   ══════════════════════════════════════════════════════════════ */
var NODES = {
  client: { x:.07, y:.50, r:12, label:'client',   sub:'',              tip:'Your process. Speaks the psql wire protocol.' },
  wal:    { x:.29, y:.19, r:15, label:'WAL',      sub:'append-only',   tip:'Write-ahead log. Every mutation lands here and is fsynced before the write is acknowledged.' },
  mem:    { x:.29, y:.62, r:18, label:'memtable', sub:'sorted, in RAM',tip:'A sorted in-memory map. Absorbs writes; flushed to an SSTable once it fills.' },
  bloom:  { x:.53, y:.62, r:13, label:'bloom',    sub:'skip filter',   tip:'One probabilistic filter per SSTable. Answers "definitely not here" without a disk seek.' },
  l0:     { x:.79, y:.16, r:14, label:'L0', sub:'6 files', lv:0, tip:'Freshly flushed SSTables. Key ranges can overlap here.' },
  l1:     { x:.79, y:.37, r:14, label:'L1', sub:'3 files', lv:1, tip:'First compacted level. Non-overlapping ranges, so at most one file per key.' },
  l2:     { x:.79, y:.58, r:14, label:'L2', sub:'2 files', lv:2, tip:'Larger, older runs. Reads here are one block read.' },
  l3:     { x:.79, y:.79, r:14, label:'L3', sub:'1 file',  lv:3, tip:'The coldest level. Tombstones can finally be dropped once nothing older holds the key.' }
};

var EDGES = [
  ['client','wal'],['client','mem'],['wal','mem'],['mem','bloom'],
  ['mem','l0'],['bloom','l0'],['bloom','l1'],['bloom','l2'],['bloom','l3'],
  ['l0','l1'],['l1','l2'],['l2','l3']
];

/* `d` is a structural fact about the engine — how many fsyncs, how many files
   touched — never a timing claim, because none of this is measured yet. */
var OPS = {
  write: { kind:'write', title:'save something', steps:[
    { from:'client', to:'wal',   t:'append to WAL',        d:'record framed and written',      dur:620 },
    { from:'wal',    to:'wal',   t:'fsync',                d:'blocks until the disk confirms', dur:900, hold:true },
    { from:'wal',    to:'mem',   t:'insert into memtable', d:'only after the log is durable',  dur:620 },
    { from:'mem',    to:'client',t:'acknowledge',          d:'the write is now crash-safe',    dur:520, hit:true }
  ], foot:'1 fsync · 1 sequential append · 0 random writes' },

  read_hit: { kind:'read', title:'find it in memory', steps:[
    { from:'client', to:'mem',   t:'probe memtable', d:'newest layer is checked first', dur:600 },
    { from:'mem',    to:'client',t:'Found → return', d:'no disk touched at all',        dur:520, hit:true }
  ], foot:'0 disk reads · 0 bloom checks' },

  read_disk: { kind:'read', title:'find it on disk', steps:[
    { from:'client', to:'mem',   t:'probe memtable',        d:'NotFound → keep going',            dur:520 },
    { from:'mem',    to:'bloom', t:'consult bloom filters', d:'one probe per SSTable, in memory', dur:520 },
    { from:'bloom',  to:'l0',    t:'L0 · definitely not',   d:'skipped, no disk I/O', dur:380, skip:true },
    { from:'bloom',  to:'l1',    t:'L1 · definitely not',   d:'skipped, no disk I/O', dur:380, skip:true },
    { from:'bloom',  to:'l2',    t:'L2 · maybe',            d:'read one block from disk',         dur:620 },
    { from:'l2',     to:'client',t:'Found → return',        d:'exactly one block read',           dur:640, hit:true }
  ], foot:'1 disk read · 3 files skipped · 4 bloom probes' },

  flush: { kind:'bg', title:'free up memory', steps:[
    { from:'mem', to:'l0',  t:'write memtable → new SSTable', d:'sorted, so it is a linear walk', dur:900 },
    { from:'l0',  to:'l0',  t:'fsync the SSTable',            d:'durable before anything else',   dur:760, hold:true },
    { from:'l0',  to:'wal', t:'clear the WAL',                d:'only now is this safe',          dur:640, hit:true }
  ], foot:'memtable emptied · WAL truncated · 1 new L0 file' },

  compact: { kind:'bg', title:'tidy up old files', steps:[
    { from:'l0', to:'l1', t:'k-way merge L0 → L1', d:'newest version of each key wins',       dur:820 },
    { from:'l1', to:'l2', t:'drop tombstones',     d:'safe once nothing older holds the key', dur:760 },
    { from:'l2', to:'l3', t:'rename into place',   d:'a crash leaves the inputs intact',      dur:760, hit:true }
  ], foot:'reads get faster · space reclaimed · inputs untouched until rename' }
};

/* ══════════════════════════════════════════════════════════════
   2. The serving path — a DESIGN, not built. Modelled on the
   split Perplexity describes for CobbleDB: durable document
   state, batched partition-aligned delivery, and a hot store
   tuned for one operation — give me these N keys, fast.
   ══════════════════════════════════════════════════════════════ */
var SNODES = {
  docs:   { x:.10, y:.14, r:15, label:'doc state',    sub:'versioned',        tip:'Durable state on cheap disk: the raw document, its passages, and every embedding version. Rebuilt from here, never re-crawled.' },
  expq:   { x:.34, y:.14, r:13, label:'export queue', sub:'partition-aligned',tip:'Queued the moment durable state changes, in the same atomic step. Groups updates by the partition that will receive them.' },
  batch:  { x:.58, y:.14, r:13, label:'batch store',  sub:'object storage',   tip:'Batches land in object storage; only a small pointer goes through the control plane. Data plane and control plane stay separate.' },
  client: { x:.06, y:.56, r:12, label:'client',       sub:'',                 tip:'Asks for a set of keys, or for the nearest vectors to a query.' },
  router: { x:.26, y:.56, r:15, label:'router',       sub:'stateless',        tip:'Hashes keys to partitions and fans out in parallel. Holds no data, so you can run as many as you like.' },
  r1:     { x:.58, y:.42, r:13, label:'replica a', sub:'same zone', lv:0, tip:'Preferred: same availability zone, so no cross-zone hop on the answer path.' },
  r2:     { x:.58, y:.64, r:13, label:'replica b', sub:'',          lv:1, tip:'A second copy of the same partition. Reads can go here if replica a is slow.' },
  r3:     { x:.58, y:.86, r:13, label:'replica c', sub:'',          lv:2, tip:'Third copy. Replicas ingest independently, so one can lag without holding up the others.' },
  cache:  { x:.84, y:.46, r:14, label:'block cache', sub:'RAM',  tip:'Hot blocks served straight from memory. You control the memory-to-disk split instead of inheriting a managed policy.' },
  nvme:   { x:.84, y:.78, r:14, label:'NVMe',        sub:'local', tip:'Local SSD for everything that misses cache. One batched read fetches many keys at once.' }
};

var SEDGES = [
  ['docs','expq'],['expq','batch'],['batch','r1'],['batch','r2'],['batch','r3'],
  ['client','router'],['router','r1'],['router','r2'],['router','r3'],
  ['r1','cache'],['r2','cache'],['r2','nvme'],['r3','nvme']
];

var SOPS = {
  batch_read: { kind:'read', title:'fetch many at once', steps:[
    { from:'client', to:'router', t:'one request, many keys', d:'a result set to rank, not a point lookup', dur:560 },
    { from:'router', to:'router', t:'group keys by partition', d:'one round trip per node, not per key',    dur:700, hold:true },
    { from:'router', to:['r1','r2','r3'], t:'fan out in parallel', d:'same-zone replica preferred',         dur:640 },
    { from:'r1', to:'cache', t:'cache hit',               d:'served from RAM, no disk touched',  dur:480, skip:true },
    { from:'r3', to:'nvme',  t:'cache miss → one batched read', d:'many keys fetched in a single call', dur:640 },
    { from:'r2', to:'client',t:'merge and return',        d:'the slowest node sets the latency',  dur:560, hit:true }
  ], foot:'1 client round trip · keys grouped by partition · disk touched only on a miss' },

  hedge: { kind:'read', title:'when a server is slow', steps:[
    { from:'client', to:'router', t:'batched read',            d:'the usual path',                       dur:520 },
    { from:'router', to:'r2',     t:'replica b is lagging',    d:'ingesting a batch, slow to answer',    dur:900, slow:true },
    { from:'router', to:'r1',     t:'hedge to replica a',      d:'fired on a timer, before b replies',   dur:520 },
    { from:'r1', to:'client',     t:'first answer wins',       d:"b's late reply is dropped",            dur:520, hit:true }
  ], foot:'tail latency bounded by the fastest replica · costs one duplicate read' },

  vector: { kind:'write', title:'find similar meaning', steps:[
    { from:'client', to:'router', t:'query vector + k',        d:'the embedding, not the text',          dur:540 },
    { from:'router', to:['r1','r2','r3'], t:'score each shard', d:'a node only scores keys it owns',     dur:700 },
    { from:'r2', to:'r2', t:'cosine against stored vectors',   d:'vectors sit beside the passage they came from', dur:820, hold:true },
    { from:'r2', to:'router', t:'partial top-k per node',      d:'k rows per node, not the whole shard', dur:600 },
    { from:'router', to:'client', t:'merge into a global top-k', d:'one pass over n·k candidates',       dur:560, hit:true }
  ], foot:'no separate vector store · the passage and its embedding share a key' },

  ingest: { kind:'bg', title:'add new pages', steps:[
    { from:'docs',  to:'expq',  t:'queue an export',            d:'same atomic step as the state write', dur:640 },
    { from:'expq',  to:'batch', t:'accumulate a partition batch', d:'one sorted file per partition',     dur:760 },
    { from:'batch', to:['r1','r2','r3'], t:'replicas pull independently', d:'a lagging node catches up at its own pace', dur:760 },
    { from:'r2',    to:'nvme',  t:'apply in order',             d:'no transaction, no synchronised replicas', dur:640, hit:true }
  ], foot:'ingest never competes with the read path · replicas may briefly disagree' },

  reindex: { kind:'bg', title:'change the AI model', steps:[
    { from:'docs',  to:'docs',  t:'write a new vector version', d:'the old one stays readable',          dur:820, hold:true },
    { from:'docs',  to:'expq',  t:'publish only the served subset', d:'the hot tier holds what search reads', dur:700 },
    { from:'expq',  to:'batch', t:'rebuild from durable state', d:'no re-crawl, no re-parse',            dur:700 },
    { from:'batch', to:['r1','r2','r3'], t:'ingest beside the old version', d:'both versions live at once', dur:700 },
    { from:'router',to:'client',t:'cut over by version',        d:'reads name a version, so the swap is atomic', dur:560, hit:true }
  ], foot:'no downtime · no re-crawl · rollback is picking the previous version' }
};

/* ══════════════════════════════════════════════════════════════
   3. Retrieval playground. The embedding is computed here, in
   your browser, by hashing words and character trigrams into a
   fixed number of dimensions and normalising — the same trick
   fastText uses for subwords. It is NOT a learned model: it
   matches spelling, not meaning. It is here to make the shape of
   the thing concrete — chunk, embed, score, rank.
   ══════════════════════════════════════════════════════════════ */
var DIM = 64;

var CORPUS = [
  { k:'wal#1',     t:'Every write is appended to the write-ahead log and fsynced to disk before the client is told it succeeded.' },
  { k:'wal#2',     t:'After a crash the log replays from the front, rebuilding the memtable exactly as it was before the kill.' },
  { k:'mem#1',     t:'The memtable is a sorted map held in RAM. Writes land here after the log; reads check it before any file.' },
  { k:'sst#1',     t:'A full memtable is flushed to an immutable sorted table on disk, written once and never modified again.' },
  { k:'bloom#1',   t:'A bloom filter answers definitely not present in constant time, so a miss costs a memory probe instead of a disk seek.' },
  { k:'compact#1', t:'Compaction merges overlapping runs, keeps the newest version of each key, and drops tombstones once nothing older holds them.' },
  { k:'raft#1',    t:'Each key range is its own raft group of three replicas, so the cluster keeps serving while a quorum survives.' },
  { k:'shard#1',   t:'Consistent hashing with virtual nodes assigns key ranges to nodes, and ranges split and merge as they grow.' },
  { k:'vec#1',     t:'Passages and their vector embeddings share one key, so retrieval reads the text and the vector in a single batched fetch.' },
  { k:'vec#2',     t:'Scoring a query vector against stored chunk embeddings by cosine similarity returns the nearest passages for ranking.' },
  { k:'pg#1',      t:'The postgres wire protocol means psql, JDBC and every ORM connect without changing a line of client code.' }
];

function hash(s) {
  var h = 2166136261;
  for (var i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619); }
  return h >>> 0;
}

/* word-level plus character-trigram features, signed-hashed into DIM dims */
function embed(text) {
  var v = new Float32Array(DIM);
  var toks = (text.toLowerCase().match(/[a-z0-9]+/g) || []);
  function add(feat, w) {
    var h = hash(feat);
    v[h % DIM] += ((h >>> 31) ? -1 : 1) * w;
  }
  for (var i = 0; i < toks.length; i++) {
    var t = toks[i];
    add('w:' + t, 1);
    var p = '<' + t + '>';
    for (var j = 0; j + 3 <= p.length; j++) add('g:' + p.substr(j, 3), 0.5);
  }
  var norm = 0;
  for (var d = 0; d < DIM; d++) norm += v[d] * v[d];
  norm = Math.sqrt(norm) || 1;
  for (var d2 = 0; d2 < DIM; d2++) v[d2] /= norm;
  return v;
}
function cosine(a, b) { var s = 0; for (var i = 0; i < DIM; i++) s += a[i] * b[i]; return s; }

var INDEX = CORPUS.map(function (c) { return { k: c.k, t: c.t, v: embed(c.t) }; });

function vecStrip(v) {
  var out = '';
  for (var i = 0; i < DIM; i++) {
    var mag = Math.min(1, Math.abs(v[i]) * 3.2);
    out += '<i style="opacity:' + (0.10 + mag * 0.9).toFixed(3) +
           ';transform:scaleY(' + (0.22 + mag * 0.78).toFixed(3) + ')"></i>';
  }
  return out;
}

function initPlayground() {
  var input = document.getElementById('vq');
  if (!input) return;
  var hits = document.getElementById('vhits');
  var strip = document.getElementById('vstrip');
  var meta = document.getElementById('vmeta');
  var kSel = document.getElementById('vk');

  function score() {
    var q = input.value.trim();
    var k = parseInt(kSel.value, 10);
    if (!q) {
      strip.innerHTML = ''; meta.textContent = 'type a query to embed it';
      hits.innerHTML = '<div class="tr-empty">Nothing to score yet.</div>';
      return;
    }
    var qv = embed(q);
    strip.innerHTML = vecStrip(qv);
    var ranked = INDEX.map(function (r) { return { k:r.k, t:r.t, s:cosine(qv, r.v) }; })
                      .sort(function (a, b) { return b.s - a.s; });
    var top = ranked.slice(0, k), max = Math.max(0.0001, top[0].s);
    meta.textContent = DIM + ' dims · ' + INDEX.length + ' chunks scored · top ' + k;
    hits.innerHTML = top.map(function (r, i) {
      var pct = Math.max(0, r.s / max) * 100;
      return '<div class="vhit' + (i === 0 ? ' best' : '') + (r.s <= 0.001 ? ' none' : '') + '">' +
             '<div class="vh-top"><span class="vh-k">' + r.k + '</span>' +
             '<span class="vh-s">' + r.s.toFixed(3) + '</span></div>' +
             '<div class="vh-bar"><i style="width:' + pct.toFixed(1) + '%"></i></div>' +
             '<div class="vh-t">' + r.t + '</div></div>';
    }).join('');
  }

  input.addEventListener('input', score);
  kSel.addEventListener('change', score);
  document.querySelectorAll('.vchip').forEach(function (c) {
    c.onclick = function () { input.value = c.dataset.q; input.focus(); score(); };
  });
  score();
}

/* ══════════════════════════════════════════════════════════════
   4. Workload model. Every number below is arithmetic on the
   sliders — there is no benchmark behind it, and it is labelled
   that way on the page.
   ══════════════════════════════════════════════════════════════ */
function fmtBytes(b) {
  var u = ['B','KB','MB','GB','TB','PB'], i = 0;
  while (b >= 1024 && i < u.length - 1) { b /= 1024; i++; }
  return (b >= 100 ? b.toFixed(0) : b >= 10 ? b.toFixed(1) : b.toFixed(2)) + ' ' + u[i];
}
function fmtNum(n) {
  if (n >= 1e9) return (n / 1e9).toFixed(1) + 'B';
  if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M';
  if (n >= 1e3) return (n / 1e3).toFixed(0) + 'k';
  return String(Math.round(n));
}

function initModel() {
  var root = document.getElementById('model');
  if (!root) return;
  var el = function (id) { return document.getElementById(id); };
  var ids = ['m-docs','m-val','m-keys','m-qps','m-hit'];

  function recalc() {
    var docs = +el('m-docs').value * 1e6;      // documents in the corpus
    var val  = +el('m-val').value * 1024;      // bytes per stored record
    var keys = +el('m-keys').value;            // keys in one query's batch
    var qps  = +el('m-qps').value * 1000;      // queries per second
    var hit  = +el('m-hit').value / 100;       // block-cache hit rate

    el('m-docs-v').textContent = fmtNum(docs) + ' docs';
    el('m-val-v').textContent  = fmtBytes(val);
    el('m-keys-v').textContent = keys + ' keys';
    el('m-qps-v').textContent  = fmtNum(qps) + ' q/s';
    el('m-hit-v').textContent  = Math.round(hit * 100) + '%';

    var corpus   = docs * val;
    var perQuery = keys * val;
    var readBw   = qps * perQuery;
    var diskBw   = readBw * (1 - hit);
    var ram      = corpus * hit;
    /* a batched read costs one round trip per partition touched; an unbatched
       client costs one per key. Partitions touched saturates at the batch size. */
    var parts    = Math.min(keys, 3);
    var saved    = keys > 0 ? (1 - parts / keys) : 0;

    el('o-corpus').textContent  = fmtBytes(corpus);
    el('o-query').textContent   = fmtBytes(perQuery);
    el('o-bw').textContent      = fmtBytes(readBw) + '/s';
    el('o-disk').textContent    = fmtBytes(diskBw) + '/s';
    el('o-ram').textContent     = fmtBytes(ram);
    el('o-rt').textContent      = parts + ' vs ' + keys;
    el('o-saved').textContent   = Math.round(saved * 100) + '% fewer';
    el('o-hitpct').textContent  = Math.round(hit * 100) + '% of reads';
    el('o-bar').style.width     = (hit * 100).toFixed(1) + '%';
  }

  ids.forEach(function (id) { el(id).addEventListener('input', recalc); });
  recalc();
}

/* ══════════════════════════════════════════════════════════════
   5. Boot
   ══════════════════════════════════════════════════════════════ */
function boot() {
  var tip = document.getElementById('cvtip');

  var engine = createTracer({
    canvas: document.getElementById('cv'),
    nodes: NODES, edges: EDGES, ops: OPS,
    stepsEl: document.getElementById('steps'),
    footEl: document.getElementById('foot'),
    hintEl: document.getElementById('hint'),
    buttons: document.querySelectorAll('.eng-bar .op'),
    tipEl: tip
  });

  var serving = document.getElementById('cv2') ? createTracer({
    canvas: document.getElementById('cv2'),
    nodes: SNODES, edges: SEDGES, ops: SOPS,
    stepsEl: document.getElementById('steps2'),
    footEl: document.getElementById('foot2'),
    hintEl: document.getElementById('hint2'),
    buttons: document.querySelectorAll('.srv-bar .op'),
    idleHint: 'click an operation to trace it through the serving path',
    tipEl: tip
  }) : null;

  /* number keys drive whichever diagram you are looking at */
  addEventListener('keydown', function (e) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    var tag = (e.target.tagName || '').toLowerCase();
    if (tag === 'input' || tag === 'textarea' || tag === 'select') return;
    var n = parseInt(e.key, 10);
    if (isNaN(n) || n < 1) return;
    var secs = [['.engine', engine], ['.serving', serving]];
    for (var i = 0; i < secs.length; i++) {
      var node = document.querySelector(secs[i][0]), t = secs[i][1];
      if (!node || !t) continue;
      var r = node.getBoundingClientRect();
      if (r.top < innerHeight * 0.6 && r.bottom > innerHeight * 0.4) {
        if (t.keys[n - 1]) { t.run(t.keys[n - 1]); e.preventDefault(); }
        return;
      }
    }
  });

  initPlayground();
  initModel();

  document.querySelectorAll('.copy').forEach(function (b) {
    b.onclick = async function () {
      try {
        await navigator.clipboard.writeText(b.dataset.copy);
        var t = b.textContent; b.textContent = 'copied';
        setTimeout(function () { b.textContent = t; }, 1200);
      } catch (err) { b.textContent = 'select manually'; }
    };
  });
}

if (document.readyState === 'loading') addEventListener('DOMContentLoaded', boot);
else boot();

})();
