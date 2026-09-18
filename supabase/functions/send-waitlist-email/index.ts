// Sends the "you're on the list" receipt for one waitlist signup.
//
// Called by the AFTER INSERT trigger in waitlist_email.sql, not by the browser.
// The trigger only fires on a genuine new row — join_waitlist() inserts with
// ON CONFLICT DO NOTHING — so a repeat signup never produces a second email.
//
// Secrets (set with `supabase secrets set`, never committed):
//   RESEND_API_KEY   from resend.com
//   WAITLIST_FROM    e.g. "youdaheDB <hello@youdahedb.dev>" — domain must be
//                    verified in Resend or everything soft-bounces
// SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY are injected by the platform.

import { createClient } from "jsr:@supabase/supabase-js@2";

const RESEND_KEY = Deno.env.get("RESEND_API_KEY");
const FROM = Deno.env.get("WAITLIST_FROM") ?? "youdaheDB <hello@youdahedb.dev>";
const SITE = Deno.env.get("SITE_URL") ?? "https://youdahedb.dev";

const admin = createClient(
  Deno.env.get("SUPABASE_URL")!,
  Deno.env.get("SUPABASE_SERVICE_ROLE_KEY")!,
);

/* Plain and text-first on purpose. A first message from an unknown domain that
   is mostly markup and images is what filters are built to catch. */
function body(email: string) {
  const text = [
    `You're on the youdaheDB waitlist.`,
    ``,
    `youdaheDB is a database being written from scratch in Rust, with an empty`,
    `[dependencies] section that stays that way. The storage engine runs today:`,
    `write-ahead log, memtable, SSTables, and crash recovery that is tested by`,
    `killing the process mid-write.`,
    ``,
    `You'll get an email when a sandbox slot opens, and a note when each layer`,
    `lands. Nothing else.`,
    ``,
    `Read along in the meantime: ${SITE}/media.html`,
    ``,
    `— Youdahe`,
    ``,
    `Signed up as ${email}. Not you? Ignore this and nothing further is sent.`,
  ].join("\n");

  const html = `<!doctype html><html><body style="margin:0;padding:24px;background:#fbfbfa;
  font:15px/1.6 -apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif;color:#08090a">
<div style="max-width:520px;margin:0 auto">
  <p style="margin:0 0 18px;font-size:17px"><strong>You're on the youdaheDB waitlist.</strong></p>
  <p style="margin:0 0 16px;color:rgba(8,9,10,.75)">youdaheDB is a database being written from scratch
    in Rust, with an empty <code>[dependencies]</code> section that stays that way. The storage engine
    runs today: write-ahead log, memtable, SSTables, and crash recovery that is tested by killing the
    process mid-write.</p>
  <p style="margin:0 0 16px;color:rgba(8,9,10,.75)">You'll get an email when a sandbox slot opens, and
    a note when each layer lands. Nothing else.</p>
  <p style="margin:0 0 24px"><a href="${SITE}/media.html"
    style="color:#08090a">Read along in the meantime &rarr;</a></p>
  <p style="margin:0 0 4px;color:rgba(8,9,10,.75)">&mdash; Youdahe</p>
  <p style="margin:24px 0 0;padding-top:16px;border-top:1px solid rgba(8,9,10,.11);
    font-size:12px;color:rgba(8,9,10,.5)">Signed up as ${email}. Not you? Ignore this and nothing
    further is sent.</p>
</div></body></html>`;

  return { text, html };
}

Deno.serve(async (req) => {
  if (req.method !== "POST") return new Response("method not allowed", { status: 405 });
  if (!RESEND_KEY) return new Response("RESEND_API_KEY is not set", { status: 500 });

  let row: { id?: string; email?: string };
  try {
    row = await req.json();
  } catch {
    return new Response("bad json", { status: 400 });
  }
  if (!row.id || !row.email) return new Response("id and email are required", { status: 400 });

  /* pg_net retries and the dashboard's "redeliver" button can both replay a
     call, so never send twice for a row that already went out. */
  const { data: existing } = await admin
    .from("waitlist").select("confirmation_sent_at").eq("id", row.id).single();
  if (existing?.confirmation_sent_at) {
    return new Response(JSON.stringify({ skipped: "already sent" }), {
      headers: { "content-type": "application/json" },
    });
  }

  const { text, html } = body(row.email);
  const res = await fetch("https://api.resend.com/emails", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${RESEND_KEY}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      from: FROM,
      to: [row.email],
      subject: "You're on the youdaheDB waitlist",
      text,
      html,
    }),
  });

  /* Record the outcome either way. A failure that is written down can be
     retried later; one that is only logged is a signup you silently ignored. */
  if (!res.ok) {
    const detail = (await res.text()).slice(0, 500);
    await admin.from("waitlist")
      .update({ confirmation_error: `${res.status} ${detail}` }).eq("id", row.id);
    return new Response(JSON.stringify({ error: detail }), {
      status: 502, headers: { "content-type": "application/json" },
    });
  }

  await admin.from("waitlist")
    .update({ confirmation_sent_at: new Date().toISOString(), confirmation_error: null })
    .eq("id", row.id);

  return new Response(JSON.stringify({ sent: row.email }), {
    headers: { "content-type": "application/json" },
  });
});
