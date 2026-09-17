# Deploying the site

Static files. No build step. Everything in `web/` is the site.

## 1. Waitlist database (Supabase)

1. Create a project at supabase.com.
2. SQL Editor → paste and run `supabase/waitlist.sql`. That creates the table,
   a case-insensitive unique index on email, and the row-level-security policy.
3. Project Settings → API → copy the **Project URL** and the **anon / public** key.
4. Put both in `web/config.js`.

The anon key ships in the page source. That is how Supabase is designed to work
— the key identifies the project, it does not grant access. **RLS is what keeps
the list private.** The policy allows `INSERT` and defines no read policy, so
that key cannot read the table back.

### Verify this before you announce the site

```sh
URL=https://YOURPROJECT.supabase.co
KEY=your-anon-key

# must print []  — an empty array, NOT rows
curl -s "$URL/rest/v1/waitlist?select=email" -H "apikey: $KEY"

# must succeed (201/204)
curl -s -o /dev/null -w '%{http_code}\n' -X POST "$URL/rest/v1/waitlist" \
  -H "apikey: $KEY" -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -H "Prefer: resolution=ignore-duplicates,return=minimal" \
  -d '{"email":"test@example.com","source":"curl"}'
```

If the first command returns rows, RLS is not on and every signup is public.
Stop and fix that before sharing the link.

Never put the `service_role` key in `config.js` — that one bypasses RLS.

### Reading the list

Supabase dashboard → Table Editor → `waitlist`, or in the SQL editor:

```sql
select email, source, created_at from public.waitlist order by created_at desc;
```

## 2. Hosting (Cloudflare Pages + Porkbun domain)

1. Push the repo to GitHub.
2. Cloudflare dashboard → Workers & Pages → Create → Pages → connect the repo.
3. Build command: *(leave empty)*. Build output directory: `web`.
4. Deploy.
5. Custom domain → add your domain. Cloudflare gives you two nameservers.
6. In Porkbun: Domain Management → Authoritative Nameservers → replace with
   Cloudflare's two. Propagation is usually minutes, up to 24h.

TLS is automatic once DNS resolves.

## 3. Before you announce

- [ ] `config.js` filled in, and the `select` curl above returns `[]`
- [ ] Sandbox passcode changed in `gate.js` (see the comment at the top for how
      to hash a new one)
- [ ] A real signup arrives in the Supabase table from the deployed site

## Known limits

- **The sandbox gate is not security.** It runs in the browser: anyone can read
  `gate.js`, set the localStorage flag, or fetch `console.html` directly. It
  keeps casual visitors behind the waitlist and nothing more. If the sandbox
  ever touches real data, the check has to move server-side.
- **The waitlist form has no CAPTCHA.** A honeypot field and the email format
  check in the RLS policy stop naive bots; a determined one can still POST to
  the endpoint directly. If it gets abused, put a Cloudflare Pages Function in
  front of it with Turnstile and move the insert behind the service role.
- **Signing up does not send email.** Nothing is wired to a mail provider — the
  table is a list you read and act on manually.
