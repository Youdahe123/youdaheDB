-- youdaheDB waitlist — run this once in the Supabase SQL editor.
--
-- The browser talks to PostgREST directly using the anon key. That key is
-- public by design and WILL be visible in the page source; what keeps the list
-- private is row-level security, not the key. The rules below let anon INSERT
-- and nothing else, so nobody can read the list back with it.

create table if not exists public.waitlist (
  id         uuid primary key default gen_random_uuid(),
  email      text        not null,
  source     text        not null default 'unknown',
  user_agent text,
  created_at timestamptz not null default now()
);

-- one row per address, case-insensitively. Signing up twice is not an error.
create unique index if not exists waitlist_email_lower_key
  on public.waitlist (lower(email));

create index if not exists waitlist_created_at_idx
  on public.waitlist (created_at desc);

alter table public.waitlist enable row level security;

-- Table grants first: RLS alone is not enough if the role has no privilege,
-- and a stray grant is not enough if RLS says no. Both have to agree.
revoke all on public.waitlist from anon, authenticated;
grant insert on public.waitlist to anon;

-- INSERT only. There is deliberately no SELECT/UPDATE/DELETE policy, so those
-- are denied for anon no matter what the client asks for.
drop policy if exists "anon can join the waitlist" on public.waitlist;
create policy "anon can join the waitlist"
  on public.waitlist
  for insert
  to anon
  with check (
    email = lower(trim(email))
    and char_length(email) between 6 and 254
    and email ~ '^[^@\s]+@[^@\s.]+\.[^@\s]+$'
    and char_length(coalesce(source, '')) <= 40
    and char_length(coalesce(user_agent, '')) <= 400
  );

-- Read the list as the project owner (service role / SQL editor), never anon:
--   select email, source, created_at from public.waitlist order by created_at desc;
