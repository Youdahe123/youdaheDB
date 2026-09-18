# Waitlist backend

Supabase holds the list; Resend sends the receipt. Supabase's own SMTP only
drives Auth templates, so arbitrary transactional mail needs a provider.

```
browser  →  join_waitlist() RPC  →  waitlist row  →  AFTER INSERT trigger
                                                  →  pg_net POST
                                                  →  send-waitlist-email
                                                  →  Resend  →  inbox
```

The browser never touches the table. `anon` has no rights on it at all — only
`execute` on one `security definer` function, which dedupes internally. The
anon key in `web/config.js` is public by design; row-level security is what
keeps the list private, not the key.

## Setup, in order

**1 · Schema** — paste each into the SQL editor, in this order:

| file | what it adds |
|---|---|
| `waitlist.sql` | the table, the case-insensitive unique index, RLS |
| `waitlist_rpc.sql` | `join_waitlist()`, and removes anon's table rights |
| `waitlist_email.sql` | delivery columns, `pg_net`, the trigger |

**2 · Resend** — create an account, add `youdahedb.dev` as a domain, and put
the DKIM/SPF records it gives you into Cloudflare DNS. Until the domain shows
as verified, mail either soft-bounces or lands in spam. Then create an API key.

**3 · Deploy the function**

```bash
supabase login
supabase link --project-ref YOUR-PROJECT-REF
supabase secrets set RESEND_API_KEY=re_xxx
supabase secrets set WAITLIST_FROM="youdaheDB <hello@youdahedb.dev>"
supabase functions deploy send-waitlist-email
```

`SUPABASE_URL` and `SUPABASE_SERVICE_ROLE_KEY` are injected by the platform —
do not set them yourself.

**4 · Vault** — the trigger needs somewhere safe to read the function URL and a
key to call it with. In the SQL editor, once:

```sql
select vault.create_secret(
  'https://YOUR-PROJECT-REF.supabase.co/functions/v1/send-waitlist-email',
  'waitlist_email_url');

select vault.create_secret('YOUR-SERVICE-ROLE-KEY', 'waitlist_email_key');
```

Until both exist the trigger is a no-op: signups are still recorded, they just
get no receipt. That is deliberate — see the comment in `waitlist_email.sql`.

## Verifying

```sql
select public.join_waitlist('you+test@yourdomain.com', 'manual-test');

select email, confirmation_sent_at, confirmation_error
  from public.waitlist order by created_at desc limit 5;

-- what pg_net actually got back
select id, status_code, content from net._http_response order by id desc limit 5;
```

`confirmation_sent_at` set means it went out. `confirmation_error` holds the
provider's response when it didn't.

## Things that will bite

- **Repeat signups send nothing.** `join_waitlist()` inserts with `on conflict
  do nothing`, so no row means no trigger. That is the intent, but it also
  means you cannot test twice with the same address — delete the row first.
- **pg_net is fire-and-forget.** The transaction does not wait for the HTTP
  call and does not roll back if it fails. A signup is never lost to a mail
  failure, but a receipt can be — which is why the outcome is written to the
  row rather than only logged.
- **Resend's free tier is 100/day, 3,000/month.** Fine for a waitlist; not
  fine if you ever loop over the whole list from here.
- **This is a receipt, not double opt-in.** Nobody has to click anything to
  stay on the list. If deliverability gets bad enough to need confirmed opt-in,
  that is a `confirmed_at` column, a token, and a route to click — a different
  change from this one.
