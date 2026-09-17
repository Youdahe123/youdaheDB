/* config.js — fill these in, then deploy.
 *
 * The anon key is MEANT to be public: it identifies the project, it does not
 * grant access. What protects the list is the row-level-security policy in
 * supabase/waitlist.sql, which lets this key INSERT and nothing else.
 *
 * Before you ship: run that SQL, then confirm the key cannot read anything:
 *   curl "$URL/rest/v1/waitlist?select=email" -H "apikey: $ANON_KEY"
 * It must come back [] — an empty array, not rows. If you see rows, RLS is off
 * and your signup list is public.
 *
 * Never put the service_role key in here. That one does bypass RLS.
 */
window.YDB_CONFIG = {
  SUPABASE_URL: 'https://bhsshtrjkemrdxfwimyr.supabase.co',
  SUPABASE_ANON_KEY: 'sb_publishable_Jx_cGGt30cFqxPzxSxjNUA_ODQAIXxT'
};
