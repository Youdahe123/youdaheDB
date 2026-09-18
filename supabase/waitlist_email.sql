-- Part 3 — run this after waitlist_rpc.sql, and after deploying the
-- send-waitlist-email Edge Function.
--
-- Fires the confirmation email on a real signup. The trigger is AFTER INSERT,
-- and join_waitlist() inserts with ON CONFLICT DO NOTHING, so a repeat signup
-- produces no row and therefore no second email. The dedupe is free.

alter table public.waitlist
  add column if not exists confirmation_sent_at timestamptz,
  add column if not exists confirmation_error   text;

-- pg_net is how Postgres makes an outbound HTTP call. It is async: the request
-- is queued and the transaction does not wait for it.
create extension if not exists pg_net with schema extensions;

-- ── secrets ──────────────────────────────────────────────────────────────
-- These live in Vault, encrypted, so the function URL and service key are
-- never written into this file or readable in the trigger body. Run once, in
-- the SQL editor, substituting your own values:
--
--   select vault.create_secret(
--     'https://YOUR-PROJECT-REF.supabase.co/functions/v1/send-waitlist-email',
--     'waitlist_email_url');
--
--   select vault.create_secret('YOUR-SERVICE-ROLE-KEY', 'waitlist_email_key');
--
-- To rotate later, use vault.update_secret(id, new_value) — not a second
-- create_secret, which would leave two rows with the same name.

create or replace function public.notify_waitlist_signup()
returns trigger
language plpgsql
security definer
set search_path = public, extensions   -- pinned, same reason as join_waitlist
as $$
declare
  v_url text;
  v_key text;
begin
  select decrypted_secret into v_url
    from vault.decrypted_secrets where name = 'waitlist_email_url';
  select decrypted_secret into v_key
    from vault.decrypted_secrets where name = 'waitlist_email_key';

  -- Not configured yet, or half-configured: take the signup anyway. Losing a
  -- subscriber because the mailer is down is a worse failure than a missing
  -- receipt, and confirmation_sent_at stays null so it can be sent later.
  if v_url is null or v_key is null then
    return new;
  end if;

  perform net.http_post(
    url     := v_url,
    headers := jsonb_build_object(
                 'Content-Type',  'application/json',
                 'Authorization', 'Bearer ' || v_key
               ),
    body    := jsonb_build_object(
                 'id',     new.id,
                 'email',  new.email,
                 'source', new.source
               ),
    timeout_milliseconds := 5000
  );

  return new;
end;
$$;

drop trigger if exists waitlist_send_confirmation on public.waitlist;
create trigger waitlist_send_confirmation
  after insert on public.waitlist
  for each row execute function public.notify_waitlist_signup();

-- anon calls join_waitlist() and nothing else; this function is not callable
-- from outside and does not need to be
revoke all on function public.notify_waitlist_signup() from public, anon, authenticated;

-- ── checking on it ───────────────────────────────────────────────────────
-- who has not been emailed, and why:
--   select email, created_at, confirmation_error
--     from public.waitlist
--    where confirmation_sent_at is null
--    order by created_at desc;
--
-- resend to everyone still missing a receipt (the function skips rows that
-- already have confirmation_sent_at, so this is safe to run twice):
--   update public.waitlist set confirmation_error = null
--    where confirmation_sent_at is null;   -- then re-fire via the dashboard,
--                                          -- or re-insert nothing and call the
--                                          -- function directly with the row id
--
-- what pg_net actually sent, newest first:
--   select id, status_code, content from net._http_response order by id desc limit 20;
