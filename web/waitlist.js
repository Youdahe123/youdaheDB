/* waitlist.js — posts a signup straight to Supabase PostgREST.
   Used by the sandbox gate and the subscribe box. No server of our own. */
(function (global) {
  'use strict';

  function cfg() {
    var c = global.YDB_CONFIG || {};
    return (c.SUPABASE_URL && c.SUPABASE_ANON_KEY) ? c : null;
  }

  /* Deliberately permissive: the real check is the regex in the RLS policy and
     an actual delivery attempt. Rejecting odd-but-valid addresses here is worse
     than accepting one that bounces. */
  function looksLikeEmail(s) {
    return /^[^@\s]+@[^@\s.]+\.[^@\s]+$/.test(s) && s.length <= 254;
  }

  /* Resolves to one of: 'ok' | 'invalid' | 'unconfigured' | 'network' | 'error'.
     Duplicates resolve to 'ok' — PostgREST is told to ignore them, so we never
     reveal whether an address was already on the list. */
  async function join(rawEmail, source) {
    var email = String(rawEmail || '').trim().toLowerCase();
    if (!looksLikeEmail(email)) return { status: 'invalid' };

    var c = cfg();
    if (!c) return { status: 'unconfigured' };

    var body = {
      email: email,
      source: String(source || 'unknown').slice(0, 40),
      user_agent: String(navigator.userAgent || '').slice(0, 400)
    };

    var res;
    try {
      res = await fetch(c.SUPABASE_URL.replace(/\/+$/, '') + '/rest/v1/waitlist', {
        method: 'POST',
        headers: {
          'apikey': c.SUPABASE_ANON_KEY,
          'Authorization': 'Bearer ' + c.SUPABASE_ANON_KEY,
          'Content-Type': 'application/json',
          /* ignore-duplicates: a second signup is a success, not a 409.
             return=minimal: we have no SELECT rights, so do not ask for the row. */
          'Prefer': 'resolution=ignore-duplicates,return=minimal'
        },
        body: JSON.stringify(body)
      });
    } catch (e) {
      return { status: 'network' };
    }

    if (res.status === 201 || res.status === 200 || res.status === 204) return { status: 'ok' };

    var detail = '';
    try { detail = (await res.text()).slice(0, 300); } catch (e) {}
    return { status: 'error', code: res.status, detail: detail };
  }

  global.YDBWaitlist = { join: join, configured: function () { return !!cfg(); } };
})(window);
