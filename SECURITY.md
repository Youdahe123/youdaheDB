# Security policy

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Use GitHub's private reporting instead: go to the
[Security tab](https://github.com/Youdahe123/youdaheDB/security/advisories/new)
and open a draft advisory. That stays private until there is a fix.

Include what you did, what happened, and what you expected. A failing test or a
short reproduction is worth more than a long description.

## Scope

This is a learning project and an early one. The storage engine is the part
worth reporting against — data loss, corruption, a crash that loses an
acknowledged write, or anything that breaks the durability claim.

Two things are **already known and deliberately in scope of "don't report"**:

- **The sandbox passcode check is not security.** It runs in the browser
  (`web/gate.js`), so anyone can read the hashes, set the localStorage flag by
  hand, or request the page directly. It keeps casual visitors behind a
  waitlist and nothing more. It guards a page of sample data, not real data.
- **The Supabase publishable key in `web/config.js` is public by design.** It
  identifies the project, it does not grant access. The waitlist table's
  row-level security allows no reads at all, and signups go through a
  `security definer` function. If you find a way to *read* the waitlist with
  that key, that is a genuine vulnerability — please report it.

## What is not in scope

- Missing rate limiting or CAPTCHA on the waitlist form. Known, documented in
  `web/DEPLOY.md`.
- Anything requiring physical access to a machine already running the database.
