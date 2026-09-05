# youdaheDB console API (v1)

The contract shared by **`web/index.html`** and **`cli/youdahedb`**. Both detect
the 404 on `/api/v1/health` and fall back to an equivalent mock engine, so the
whole UX is usable and demoable before any of this is implemented.

This is the **end-state** contract — most of it maps to milestones that are not
built. Each section is tagged with its milestone and issues so the surface can
be filled in incrementally.

All responses JSON. Base path `/api/v1`.

---

## `GET /health` — *M3 · [#19](https://github.com/Youdahe123/youdaheDB/issues/19)*

```json
{ "ok": true, "version": "0.1", "node_id": "n1" }
```

Presence of this endpoint is what flips both clients from mock to live.

---

## `POST /query` — *M7 · [#42](https://github.com/Youdahe123/youdaheDB/issues/42) [#43](https://github.com/Youdahe123/youdaheDB/issues/43)*

Request `{ "sql": "select id, email from users limit 10" }`

Result set:
```json
{ "cols": ["id","email"], "rows": [[1,"a@b.c"]], "ms": 0.42 }
```
`EXPLAIN` returns a plan instead of rows: `{ "plan": "<text>", "ms": 0.18 }`
Statement with no result set: `{ "notice": "INSERT 0 1", "ms": 0.31 }`
Error: `{ "error": "relation \"x\" does not exist", "ms": 0.02 }`

> Only `error` is a client-visible failure; the HTTP status stays 200 so the
> shell can render a Postgres-style `ERROR:` line rather than a transport fault.

---

## `GET /schema` — *M7 · [#42](https://github.com/Youdahe123/youdaheDB/issues/42)*

```json
{ "database":"youdahedb",
  "tables":[ { "name":"users", "pk":"id", "rows":5,
               "columns":[["id","bigint","not null"]] } ] }
```

---

## `GET /storage` — *M1 · [#3](https://github.com/Youdahe123/youdaheDB/issues/3) [#4](https://github.com/Youdahe123/youdaheDB/issues/4) [#5](https://github.com/Youdahe123/youdaheDB/issues/5) [#8](https://github.com/Youdahe123/youdaheDB/issues/8) [#10](https://github.com/Youdahe123/youdaheDB/issues/10)*

The only section whose backing code is close to existing.

```json
{ "memtable": { "entries":41, "bytes":1180, "limit":64 },
  "wal":      { "bytes":1180, "segments":1 },
  "levels":   [ { "level":0, "files":6, "entries":49120, "bytes":12600000 } ],
  "totals":   { "files":12, "bytes":562600000, "entries":2073120 },
  "bloom":    { "checks":184220, "skipped":151004, "fp_rate":0.0097 },
  "manifest": { "records":2114, "live_files":12 } }
```

`levels` is ordered L0→Ln. `bloom.skipped` is the count of SSTables a read never
opened — the number that justifies [#5](https://github.com/Youdahe123/youdaheDB/issues/5).

---

## `GET /nodes` — *M4/M5 · [#23](https://github.com/Youdahe123/youdaheDB/issues/23)–[#28](https://github.com/Youdahe123/youdaheDB/issues/28) [#33](https://github.com/Youdahe123/youdaheDB/issues/33)*

```json
{ "nodes":[ { "id":"n1", "addr":"10.0.1.11:6380", "region":"us-east-1a",
              "role":"leader", "status":"live", "uptime":"4d 02:11",
              "ranges":6, "cpu":31, "store":"412 MB", "term":7 } ] }
```
`status` ∈ `live | suspect | dead` — drives the status palette in both clients.

---

## `GET /ranges` — *M5 · [#31](https://github.com/Youdahe123/youdaheDB/issues/31) [#32](https://github.com/Youdahe123/youdaheDB/issues/32) [#34](https://github.com/Youdahe123/youdaheDB/issues/34)*

```json
{ "ranges":[ { "id":"r2", "start":"/Table/users/1", "end":"/Table/users/5000",
               "lease":"n1", "replicas":["n1","n2","n3"],
               "size":"128 MB", "qps":1180, "term":7 } ] }
```
`replicas[0]` is the leaseholder. Ring arcs are coloured by leaseholder node.

---

## `GET /txns` — *M6 · [#36](https://github.com/Youdahe123/youdaheDB/issues/36)–[#41](https://github.com/Youdahe123/youdaheDB/issues/41)*

```json
{ "txns":[ { "id":"txn-8f21a", "status":"pending", "isolation":"serializable",
             "hlc":"1756982411.0003", "age":"142ms",
             "ranges":2, "writes":14, "node":"n1" } ] }
```
`status` ∈ `pending | committed | aborted`. `hlc` is the hybrid logical clock
timestamp from [#38](https://github.com/Youdahe123/youdaheDB/issues/38).

---

## `GET /jobs` — *M1/M5/M8 · [#4](https://github.com/Youdahe123/youdaheDB/issues/4) [#17](https://github.com/Youdahe123/youdaheDB/issues/17) [#34](https://github.com/Youdahe123/youdaheDB/issues/34) [#48](https://github.com/Youdahe123/youdaheDB/issues/48)*

```json
{ "jobs":[ { "id":"job-114", "type":"COMPACTION", "desc":"L0→L1 merge",
             "status":"running", "pct":62, "node":"n1", "started":"00:01:12" } ] }
```
`type` ∈ `COMPACTION | REBALANCE | BACKUP`,
`status` ∈ `running | queued | succeeded | failed`.

---

## `GET /metrics` — *M8 · [#45](https://github.com/Youdahe123/youdaheDB/issues/45) [#46](https://github.com/Youdahe123/youdaheDB/issues/46)*

```json
{ "window":"60m",
  "series": { "qps": { "label":"Queries per second", "unit":"qps",
                       "data":[1402.1, 1411.8] } } }
```

One measure per series — the console renders each as its own chart and never
puts two scales on one axis. `GET /metrics` in **Prometheus text format** is a
separate endpoint at the server root (`/metrics`, not under `/api/v1`).

---

## `GET /` — *M3*

Serves the console. Embed with `include_str!("../web/index.html")` so the
server stays a single binary with no asset directory to ship.
