/* gate.js — sandbox access check.
 *
 * READ THIS BEFORE YOU RELY ON IT: this is a soft gate, not security. The
 * whole check runs in the visitor's browser, so anyone who opens devtools can
 * read this file, set the localStorage flag by hand, or just request
 * console.html directly. It keeps casual visitors behind the waitlist; it does
 * not protect anything. The moment the sandbox touches a real backend, the
 * check has to move to the server.
 *
 * To change the passcode, hash the lowercased code and put it in CODES:
 *   node -e "function h(s){let x=2166136261;for(let i=0;i<s.length;i++){x^=s.charCodeAt(i);x=Math.imul(x,16777619)}return(x>>>0).toString(16)};console.log(h('your-code'))"
 */
(function (global) {
  'use strict';
  var KEY = 'ydb-access';

  /* hashes of the accepted passcodes, lowercased */
  var CODES = [
    'a28f34e1',   // YDB-EARLY-2026
    'b571cc79'    // LSM2026
  ];

  function hash(s) {
    var x = 2166136261;
    for (var i = 0; i < s.length; i++) { x ^= s.charCodeAt(i); x = Math.imul(x, 16777619); }
    return (x >>> 0).toString(16);
  }

  global.YDBGate = {
    check: function (code) {
      return CODES.indexOf(hash(String(code).toLowerCase().trim())) > -1;
    },
    unlocked: function () {
      try { return localStorage.getItem(KEY) === '1'; } catch (e) { return false; }
    },
    unlock: function () {
      try { localStorage.setItem(KEY, '1'); } catch (e) {}
    },
    lock: function () {
      try { localStorage.removeItem(KEY); } catch (e) {}
    },
    /* called in <head> on a gated page, before anything renders */
    require: function () {
      if (this.unlocked()) return;
      var here = location.pathname.split('/').pop() || 'home.html';
      location.replace('access.html?next=' + encodeURIComponent(here + location.search + location.hash));
    }
  };
})(window);
