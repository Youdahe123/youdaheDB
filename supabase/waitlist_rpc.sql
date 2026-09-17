-- Part 2 — run this after waitlist.sql.
--
-- Why this exists: PostgREST's "ignore duplicates" needs SELECT on the table,
-- and granting anon SELECT would make every signup publicly readable. So the
-- insert moves into a SECURITY DEFINER function instead. The function runs as
-- its owner, so it can dedupe internally, while anon keeps no access to the
-- table at all — it can only call this one function.

create or replace function public.join_waitlist(
  p_email      text,
  p_source     text default 'unknown',
  p_user_agent text default null
)
returns void
language plpgsql
security definer
set search_path = public    -- pinned: a definer function without this is hijackable
as $$
declare
  v_email text := lower(trim(p_email));
begin
  if v_email !~ '^[^@\s]+@[^@\s.]+\.[^@\s]+$'
     or char_length(v_email) not between 6 and 254 then
    raise exception 'invalid email' using errcode = '22023';
  end if;

  insert into public.waitlist (email, source, user_agent)
  values (v_email, left(coalesce(p_source, 'unknown'), 40), left(p_user_agent, 400))
  on conflict do nothing;    -- signing up twice is a success, not an error
end;
$$;

-- anon may call this and nothing else
revoke all on function public.join_waitlist(text, text, text) from public;
grant execute on function public.join_waitlist(text, text, text) to anon;

-- and now anon needs no direct table rights whatsoever
revoke all on public.waitlist from anon, authenticated;
drop policy if exists "anon can join the waitlist" on public.waitlist;
