# Providers (Windows)

Windows rewrite of the *role* of upstream `docs/providers.md`: how providers are registered and fetched in **this** repo.
Do **not** treat upstream’s full strategy table as authoritative for Win-CodexBar without checking code — IDs and auto-order drift.

## Single factory

All shells and the CLI construct providers through:

```text
codexbar::core::instantiate_provider  →  rust/src/core/provider_factory.rs
```

`ProviderId` lives in `rust/src/core/provider.rs`. The factory match is **exhaustive** (missing arm = compile error). Tests ensure every id instantiates.

**Never** duplicate provider factories in the Tauri shell or ad-hoc commands.

## Adding a provider

1. Add a `ProviderId` variant + `cli_name` / `display_name` / cookie domain / `from_cli_name` metadata as required.
2. Implement `Provider` in `rust/src/providers/<name>/` (or module).
3. Add the match arm in `provider_factory.rs::instantiate`.
4. Keep provider-specific parsing and auth **inside** that module — no cross-provider branching in shared UI paths.
5. Keep identity / plan / email **siloed** per provider in the UI.

## Fetch strategies (concept)

Same vocabulary as upstream, implemented in Rust:

| Source label | Meaning (typical) |
|--------------|-------------------|
| `auto` | Provider-specific fallback order |
| `web` | Cookie / dashboard HTTP |
| `cli` | Local CLI / PTY / RPC helpers |
| `oauth` | OAuth-backed flows where supported |

CLI: `codexbar usage --source auto|web|cli|oauth`.

Auth resolution helpers in `rust/src/providers/` commonly try: explicit settings → keyring/entry → environment variables (exact order is provider-specific).

### Devin manual authentication

On Windows, Devin uses a manually pasted Bearer token; Chrome-session import is
not available. In Devin, open Developer Tools → Network, reload **Usage &
Limits**, then copy the `Authorization` value from a successful
billing/quota/usage request. In Settings → Providers → Devin, paste a bare
token, a `Bearer ...` value, or the full `Authorization: Bearer ...` line into
the token field. Never share this token.

Set **Organization** to the internal `org-...` or `org_...` ID, an organization
slug, or a `devin.ai` organization URL. The internal ID from the
`x-cog-org-id` header on a successful quota request is the most direct choice.
Environment variables are `DEVIN_BEARER_TOKEN`, `DEVIN_AUTHORIZATION`, or
`DEVIN_API_KEY` for the token, and `DEVIN_ORGANIZATION` or `DEVIN_ORG` for the
organization.

## Cookie-backed providers

Windows browser import: Chrome Stable, Beta, Dev, Canary, Chrome for Testing, Chromium, Edge, Brave (DPAPI + AES-GCM), and Firefox (SQLite). Chrome channels are detected separately and keep distinct import choices.
Settings → **Providers** → provider detail → choose browser → Import.  
Manual cookie header paste is the fallback (required under WSL for Chromium DPAPI).  
Details: [COOKIES.md](./COOKIES.md).

### Venice web session

Venice normally uses an API key (USD / DIEM balance). The optional web mode
reads the signed-in `venice.ai` Clerk session instead and reports the
bundled-credit quota from the session token.

- Web (automatic) mode needs a signed-in `venice.ai` tab in a supported
  browser, because the session token comes from the `__session` cookie.
- Clerk sessions last only about 60 seconds, so Web mode does not refresh
  unattended. When the session has expired, open `venice.ai` in the browser
  again and refresh.
- Manual mode needs a freshly pasted Cookie header from a signed-in
  `venice.ai` request; an old header fails with the expired-session message.
### Raycast credits

Raycast reads the unofficial `frontend_api/current_user/ai_credits` route of
`www.raycast.com` with the `__raycast_session` and `csrf_token` cookies only.
Auto imports those cookies from Chrome (all profiles, exact `www.raycast.com`
host first); Windows Chrome App-Bound Encryption can block automatic import, in
which case paste a Cookie header under Manual. A manual header is pinned and
never falls back to a browser, and Off makes no request. When the response has
a positive allowance and a known balance the provider shows one "credits left"
meter with the renewal date; otherwise it shows the balance and total as detail
rows. Top-up packages and credit details are not fetched.

### Replicate billing

Replicate uses the signed-in `replicate.com` session cookie for its billing
page and read-only account endpoints. Automatic mode reuses a validated local
cookie before importing the browser session; manual mode accepts a Cookie
header containing a nonempty `sessionid`. The provider reports this month's
spend and, when the optional balance request succeeds, prepaid credit balance.
It keeps those values in the cost/detail surfaces and does not invent a quota
percentage or use a Replicate API token as a website credential.

### Charm Hyper balance

Charm Hyper is disabled by default. It reads
`GET https://hyper.charm.land/v1/credits` and shows one **Hypercredits** detail
row in native HC units; it never infers a quota percentage, plan, reset or
spend. The cookie source only picks the session: Automatic imports the
`hyper.charm.land` session from the selected browser, Manual uses a pasted
Cookie header, and Disabled uses only the API key. The usage source keeps
routing. Auto prefers the session and falls back to the API key (the saved key,
then `HYPER_API_KEY`) when there is no session, the session request fails, or
the session is rejected (401, 403, a redirect or an HTML sign-in page).
Browser session never uses the key, and API never reads cookies. The cookie and
the bearer key are never sent together. Rate limits, server errors and
malformed balances are final and do not fall back. Upstream's multiple API-key
token accounts are not ported yet.

### Langdock personal usage

Langdock is disabled by default and has no API key. It reads the signed-in web
app: `GET https://app.langdock.com/api/trpc/usageSettings.getPersonalUsage`
(tRPC batch input) with the `auth_token` session cookie.

- The cookie source in Settings → Providers → Langdock picks the session and
  defaults to Automatic. Automatic and Manual first use a header imported under
  Browser Cookies (from the browser you choose) or pasted there. Without one,
  Automatic uses the first detected browser whose `langdock.com` cookies hold
  `auth_token` (browsers with only other cookies are skipped) and Manual fails
  closed. Off reads nothing. A header without `auth_token` never sends a
  request.
- The card shows Session (5 hours) and Weekly percentages with their resets.
  A plan without session limits shows the weekly lane alone, labelled Weekly.
  A workspace without included limits shows one "Included limits" row with
  "No included usage limits available".
- Upstream keeps usage above 100%; the Windows bar clamps it at 100%.
- 401 and tRPC `UNAUTHORIZED` are reported as an expired session. Redirects
  are never followed and response bodies are never echoed in errors.

### LithosAI prepaid balance

LithosAI is disabled by default and has no billing API key. It reads the
signed-in console at `https://console.lithosai.cloud` with both session cookies,
`__Host-console_session` and `__Host-console_csrf`; the CSRF value is echoed as
`X-Console-Csrf`. A header missing either cookie never sends a request.

- The cookie source in Settings → Providers → LithosAI picks the session and
  defaults to Automatic. Automatic and Manual first use a header imported under
  Browser Cookies (from the browser you choose) or pasted there. Without one,
  Automatic tries each detected browser holding both `console.lithosai.cloud`
  cookies, in order, and moves to the next after a 401; Manual fails closed
  and a pasted header is never swapped for a browser session. Off reads nothing.
- Requests, in order: `GET /api/me` (active organization, email), then
  `GET /api/billing` and `GET /api/billing/spend?start=YYYY-MM-01&end=YYYY-MM-DD`
  (UTC month to date), both with `X-Organization-Id`.
- The card shows the prepaid USD balance (1 USD = 1,000,000,000 nanos) as a
  "Prepaid credits" cost block and a **Billing** section: Balance, Payment card,
  Account status, Today (UTC) and This month (UTC). There is no quota bar. A
  negative balance has no cost block and stays visible as, for example,
  "Balance: -$1.00" in the primary line and the Balance row.
- A spend report that fails or does not parse shows "Spend: Unavailable" and
  keeps the balance; a 401 anywhere rejects that session, and an expired
  session is reported once no candidate is left.
- Response bodies and cookies are never echoed in errors or logs.

### Muse (muse.ai) weekly usage

Muse (muse.ai) is Meta's personal agent; it is separate from Muse Code, which
reads the `muse` CLI login. It is disabled by default and has no usage API, so
it posts the settings dialog's Next.js server action `fetchSubscriptionAction`
to `https://muse.ai/` with the session cookies and `Sec-Fetch-*` headers.

- The cookie source in Settings → Providers → Muse (muse.ai) picks the session
  and defaults to Automatic. Automatic and Manual first use a header imported
  under Browser Cookies (from the browser you choose) or pasted there. Without
  one, Automatic reads the `muse.ai` cookies from the first detected browser
  that has them and Manual fails closed. Off reads nothing.
- The action ID changes on every deploy. A `404 Server action not found.`
  starts a bounded discovery: the signed-in page's chunks (at most 96, eight
  at a time, 8 MiB in total) lead to the lazy settings module, whose chunks
  (at most 32) hold the new ID. Static chunks receive no cookies. The ID is
  kept in memory after a successful response; upstream stores it on disk.
- The card shows the weekly percentage and reset, the plan, the paid-plan
  "tokens left" text, the renewal date and any top-up tokens as a detail row
  with a progress bar.
- A 401, a 403 or a redirect to `auth.muse.ai` is reported as an expired
  session. Response bodies and cookies are never logged or echoed in errors.

### TinyApi

TinyApi reads available credit balance from the signed-in dashboard using
`GET https://tinyapi.rest/api/user/credits` with session cookies or a manual
Cookie header. The response returns a JSON object with a `data.totalAvailable`
field containing the available credits as a number.

- The cookie source in Settings → Providers → TinyApi picks the session and
  defaults to Automatic. Automatic imports the `tinyapi.rest` cookies from the
  browser you select (Chrome, Edge, Brave, Firefox); Manual accepts a pasted
  Cookie header. Without an imported or pasted header, Automatic tries each
  detected browser's `tinyapi.rest` cookies in turn, and Manual fails closed.
  Off reads nothing.
- Requests send the Cookie header and expect a JSON response with `success: true`
  and `data.totalAvailable` as a finite non-negative number. HTTP 401 or 403
  indicates an expired session; the provider skips to the next browser session
  if Automatic has more candidates, and reports "TinyApi session expired. Sign
  in again or paste a fresh Cookie header." if none remain. HTTP 429 is
  rate-limited, and 500+ is unavailable. Other HTTP codes are reported with
  their status code. Bodies over 256 KiB or without a valid balance report a
  missing balance, and non-JSON bodies report invalid JSON. Response bodies and
  cookies are never logged or echoed in errors.
- The card shows a single "Available credits" detail row with the balance
  formatted to up to two decimal places and the "credits" unit (e.g. "125.5
  credits" or "100 credits"). The menu bar reads "Available credits: <value>"
  from an informational window that is hidden on the card. Login method is
  shown as "Browser session".
- No multi-account support. A transport or timeout error while reading the
  body keeps its cause instead of being reported as an invalid response.

### WorkBuddy credits

WorkBuddy is Tencent's desktop AI work assistant, billed in credits. It is
disabled by default and reads the website billing API that the signed-in
Plans & usage page (`https://www.workbuddy.cn/profile/plans-usage`) uses:
`POST /billing/meter/get-user-resource-summary` with the session cookies.

- The cookie source in Settings → Providers → WorkBuddy picks the session and
  defaults to Automatic. Automatic and Manual first use a header imported under
  Browser Cookies (from the browser you choose) or pasted there, with no
  browser fallback. Without one, Automatic tries each detected browser's
  `workbuddy.cn` session in turn (only the cookies a request to
  `www.workbuddy.cn` would carry) and Manual fails closed. Off reads nothing.
- WorkBuddy binds the session to the browser User-Agent. Requests send
  Chrome's reduced Windows User-Agent for the installed Chrome major (the
  newest version folder under the Chrome install, else the version in
  `HKCU\Software\Google\Chrome\BLBeacon`) and retry once with the previous
  major. Without a Chrome version no User-Agent is sent, which WorkBuddy
  usually rejects as an expired session, so cookies should come from Chrome.
- The card shows the credits meter with "X / Y credits left", the plan name,
  reserved (frozen) credits, and the cycle reset from the paid and free
  package listings (cycle ends are China Standard Time; the listings share a
  5-second budget and a failed listing only drops the reset).
- HTTP 401 on every User-Agent moves on to the next browser session and is
  reported as an expired session at the end, unless Automatic could not read
  a browser (for example Chrome behind App-Bound Encryption): then that read
  error is reported instead (App-Bound Encryption is also reported when no browser holds a session).
  403, 429, server errors,
  non-zero API codes and malformed payloads stop at once. Response bodies and
  cookies are never logged or echoed in errors.

### llmman daemon

llmman reads a local (or LAN) llmman daemon: `GET {base}/llmman/node` for the
memory budget plus loaded and stored models, and a best-effort
`GET {base}/api/version`. The base URL comes from provider extras or
`LLMMAN_HOST` and defaults to `http://127.0.0.1:17434`; a trailing `/v1` is
accepted and dropped per request. `LLMMAN_API_KEY` is optional, because a
daemon without configured keys is open, and is sent as a Bearer token only when
set. Plain HTTP is accepted only for localhost, loopback, private-network
(10/8, 172.16/12, 192.168/16, 169.254/16, fc00::/7, fe80::/10) and `.local`
hosts; any public host must use HTTPS. Embedded credentials, queries, and
fragments are rejected.

### DevPass

DevPass reads `GET https://api.llmgateway.io/v1/key` with a regular LLM Gateway
API key (`DEVPASS_API_KEY` or the Settings key field). Publishable keys and
end-user sessions get HTTP 403. Plan credits are the primary lane and the
premium weekly allowance is the secondary lane (seven-day window; the reset
comes only from the response, and an inactive window has no reset). A
pay-as-you-go key (`devPlan: none`) shows only key-scoped all-time spend.
Remaining plan credits are an allowance, not a wallet balance, so no balance is
reported. Amounts must be plain decimal strings; anything else fails the
refresh instead of showing as zero. The key is only sent to the fixed HTTPS
origin, redirects are not followed, and response bodies never appear in errors.

### Muse Code browser team quota

Muse Code reads its quota from the device-code login. When that response has no
`subs_usage`, CodexBar can optionally read the quota of one dev.meta.ai team you
select in Settings → Providers → Muse Code → **Browser team**.

- The cookie source is **Off** by default. The Windows default ("Manual" with no
  pasted cookie) also reads nothing, and never touches a browser.
- **Automatic** imports the `llama_dev_sess` cookie from the selected browser.
  **Manual** accepts a pasted Cookie header or cURL capture.
- Requests, in order: `GET /api/auth/me` (its email must match the login email,
  case-insensitively), `GET /api/portal/teams`, then
  `GET /api/portal/teams/{id}/subscription-quota` for the selected team only. The
  team's `tier` must equal the login plan. At most 5 requests, 8 seconds each.
- The team list appears as the picker options and starts at "Choose a team...";
  the first team is never selected for you. The team ID is stored as the provider
  workspace value and accepts digits only.
- The source label becomes `oauth+web`. This port has no `estimated` data
  confidence, so the pace shown for this reading is marked non-authoritative
  instead. The device-code token is only sent to `api.meta.ai`; dev.meta.ai
  requests carry only the browser cookie.
- The CLI does not read browser cookies for Muse Code.

## API-key gateway providers

### Aixy

Aixy reports the API key's own usage and the budgets that apply to it
(`GET {base}/v1/usage`, `Authorization: Bearer <key>`). Configure the key in
Settings → Providers → Aixy (or a token account, or `AIXY_API_KEY`). Leave the
Base URL empty for `https://api.aixy-gateway.com`; set it (or `AIXY_BASE_URL`)
for a self-hosted gateway. A trailing `/v1` and path prefix are accepted; HTTPS
is required except for localhost, private-network and `.local` hosts, and a
Base URL with embedded credentials, a query or a fragment is rejected. Redirects
are never followed and response bodies are never echoed in errors.

- Hard budgets come first; the two most-utilised known budgets fill the primary
  and secondary lanes and every other budget is a named window. Overlapping
  budgets are never summed. Unknown balances show as Unavailable.
- Details show the key, project and observation time, each applicable budget,
  and the last seven days of requests, tokens, attributed spend and coverage.
  Attributed spend is the provider cost; it is not an invoice.
- Only the Automatic menu-bar metric is offered.
- A response that fails the `key.usage` contract (wrong currency, another key's
  budget, inconsistent coverage, malformed amounts) is a parse error, not a
  partial balance.

### Synthetic

Synthetic reads `GET https://api.synthetic.new/v2/quotas` with
`Authorization: Bearer <key>`. Configure the key in Settings → Providers →
Synthetic (or `SYNTHETIC_API_KEY`); the CLI also accepts `synthetic.new`.

- `rollingFiveHourLimit` is the "Five-hour quota" lane, `weeklyTokenLimit` the
  "Weekly tokens" lane and `search.hourly` a "Search hourly" window. Other
  payload shapes fall back to the first quota-like objects found under
  `quotas`, `quota`, `limits`, `usage`, `entries`, `subscription` or `data`.
- When the five-hour quota reports `tickPercent`, the lane resets at the next
  regen tick and its detail line reads, for example,
  "76% after next regen · Full in ~3 regens". The weekly lane does the same
  when it carries `maxCredits` and `nextRegenCredits`. Like the Mac card, no
  weekly USD cost is shown (on the card or in CLI JSON).
- The plan name (`plan`, `planName`, `tier`, ...) is shown as the plan.
- 401 and 403 report "Invalid Synthetic API credentials."; response bodies are
  never echoed in errors.

### Sail Research

Sail Research reads `GET https://api.sailresearch.com/v2/usage/summary?range=30d`
with bearer authentication. Configure the API key in Settings → Providers →
Sail Research (or `SAIL_API_KEY`).

- The "Organization billing" section shows the credit balance and the spend for
  the effective range the API reports (for example "Last 30 days spend").
  Amounts arrive as fractional cents and display as USD.
- When `balance_unavailable` is true the balance row shows "Unavailable" and
  the spend row is kept.
- 401 reports "Sail Research returned HTTP 401."; 402 reports exhausted
  credits; 403 reports missing organization access. A billing summary that is
  unavailable, has no billing account, or fails validation is reported as an
  error. Response bodies are never echoed in errors.

### Linkup

Linkup reads `GET https://api.linkup.so/v1/credits/balance` with bearer
authentication. Configure the API key in Settings → Providers → Linkup (or
`LINKUP_API_KEY`).

- The "Account balance" section shows one "Credit balance" row in USD. Zero
  and negative balances are shown as reported.
- Every non-200 status reports "Linkup returned HTTP <status>."; a body that is
  not an object with a finite numeric `balance` reports "Linkup returned an
  unrecognized credit balance.". Response bodies are never echoed in errors.

### Sofya

Sofya reads `GET https://sofya.co/v1/auth/me` with bearer authentication.
Configure the API key in Settings → Providers → Sofya (or `SOFYA_API_KEY`).

- The "Account credits" section shows "Available credits" (`credits`),
  "Plan credits" and "Purchased credits" when present, with up to six decimals.
  At least one must be present; negative or non-numeric values fail closed.
- `is_free_tier` sets the login label: "Free tier", "Pay as you go", or "API"
  when absent. A free-tier account with `credits_reset_at` also shows
  "Monthly reset" in UTC. The reset timestamp must include a time zone and is
  validated even when it is not shown.
- Every non-200 status reports "Sofya returned HTTP <status>."; other account
  fields (email, API key, usernames) are never read or displayed.

### Tavily

Tavily reads `GET https://api.tavily.com/usage` with bearer authentication.
Configure the API key in Settings → Providers → Tavily (or `TAVILY_API_KEY`).

- The "Account plan" lane reads `account.plan_usage` / `account.plan_limit`.
  The "API key" lane reads `key.usage` / `key.limit`. A null key limit means
  Unlimited; zero limits and unlimited keys display their rows without a
  usage percentage. Remaining credits are the nonnegative difference between
  reported cap and usage.
- Pay-as-you-go totals, when present (`account.paygo_usage` and
  `account.paygo_limit`), appear in a separate detail section.
- The plan name (`account.current_plan`) identifies the account plan. No email
  or account ID is inferred; unknown plans do not create a phantom identity.
- Non-200 responses report "Tavily returned HTTP <status>."; response bodies
  are never echoed in errors.
- Tavily limits the usage endpoint to 10 requests per 10 minutes. Keep
  refreshes at least one minute apart and avoid repeated manual refreshes. A
  throttled request fails as rate-limited without automatic replay.

### Exa

Exa reads selected API key monthly spend from `GET https://admin-api.exa.ai/team-management/api-keys/{id}/usage`
with Team Management service key authentication (`x-api-key` header). Request dates are rounded down to whole
seconds. Configure both the service key and the API key ID in Settings → Providers → Exa (or `EXA_SERVICE_KEY`
and `EXA_API_KEY_ID`).

- Exa requires Team Management API access, enabled per-team by Exa support (`support@exa.ai`). Create a service
  account API key under **Service keys** on the API keys page; ordinary search API keys are not supported.
- The scope field (API key ID) is the identifier of the API key whose spend you want to monitor, not its secret.
  Both keys must belong to the same team. API key IDs reject empty strings and path normalization values ("." and "..").
- The "Spend" lane shows selected-key month-to-date USD (UTC calendar month start to request time). Zero spend
  stays visible. Window percentage is a placeholder; only the detail section is meaningful.
- No team balance, allowance, reset date, or quota is inferred. Account history and burn-down widgets are
  unavailable because the plugin does not expose a stable account identity. No browser cookies are imported.
- 401 reports "Exa rejected the service key. Ordinary search API keys are not supported."; 403 reports "Exa Team
  Management must be enabled and both keys must belong to the same team." Response bodies are never echoed in
  errors. Malformed, nonfinite, negative, or period-mismatched spend values fail parsing.

### ClawRouter

ClawRouter reports the monthly budget, spend and routed-provider usage of one
policy key (`GET {base}/v1/usage`, `Authorization: Bearer <key>`). Configure
the key in Settings → Providers → ClawRouter (or `CLAWROUTER_API_KEY`). Leave
the Base URL empty for `https://clawrouter.openclaw.ai`; set it (or
`CLAWROUTER_BASE_URL`) for another deployment. The service root and its `/v1`
URL both work, a bare host is treated as HTTPS, and plain HTTP, embedded
credentials, a query or a fragment are rejected before the key is read.
Redirects are never followed and response bodies are never echoed in errors.

- The primary lane is the monthly budget (spent / limit), resetting on the
  first day of the next month (UTC). The cost line shows this month's spend
  against the limit.
- Details show requests, tokens, actual cost, the budget ledger and the
  monthly budget, then up to 20 routed providers ordered by spend and
  requests.
- A policy without a monthly limit shows "Unmetered" and this month's actual
  cost.

### IBM Bob

IBM Bob reads monthly Bobcoin usage. Configure an API key from the IBM Bob web
portal in Settings → Providers → IBM Bob, as token accounts, or with
`BOBSHELL_API_KEY`; the CLI also accepts `ibm-bob`, `bob` and `bobshell`.

- `GET https://api.us-east.bob.ibm.com/admin/v1/profile` lists the
  subscription instances; then each team's budget comes from
  `GET /admin/v1/teams/{team}/users/{user}` on the instance's regional host.
- Credentials go only to HTTPS hosts named `bob.ibm.com` or under
  `.bob.ibm.com`; any other `region_domain` stops the refresh before a team
  request. A JWT session token is sent as `Bearer`, any other key as `Apikey`.
- The "Monthly Bobcoins" bar sums every visible team and shows a percentage
  only when every team has a budget; otherwise it shows Bobcoins used. It
  resets at the earliest `refresh_at`. The "Bobcoin usage" rows list each team
  with its plan, and the plan names are shown as the organization.
- 401 and 403 report that the key was rejected; response bodies are never
  echoed in errors.

### X API prepaid credits

X API is disabled by default and reads prepaid credit balances from the X
(Twitter) API console. Sign in to console.x.com in your browser, and CodexBar
will read the `auth_token` and `ct0` cookies from that session.

- **Automatic:** A previously imported Cookie header is used on its own.
  Otherwise CodexBar tries each detected browser (Chrome, Edge, Brave,
  Firefox) holding both cookies. If a browser session expires (401), it moves
  to the next browser. No session means sign in to console.x.com.
- **Manual:** Paste the Cookie header from a console.x.com request (with
  `auth_token` and `ct0`). Manual mode does not fall back to browser sessions.
- **Off:** Cookies are disabled.

The primary lane shows the balance (paid + free credits). Details rows show:
- Balance (total: paid + free)
- Purchased credits
- Free credits

Balance is a prepaid meter (no quota percentage); costs are always zero (no
usage tracking available). The provider reports identity as "Browser session".
Cost resets are never inferred. Session rotation on 401 is implemented: sessions
are tried in browser order, stale sessions are skipped, and a missing session
after all attempts triggers "sign in to console.x.com". Errors never echo
response bodies.

### Parallel

Parallel is disabled by default and reads the prepaid account balance from the
Parallel platform console. Sign in to platform.parallel.ai in your browser, and
CodexBar will use that session's cookies.

- **Automatic:** A previously imported Cookie header is used on its own.
  Otherwise CodexBar tries each detected browser (Chrome, Edge, Brave, Firefox)
  holding a platform.parallel.ai session. If a session is signed out, it moves
  to the next browser. No session means sign in to platform.parallel.ai.
- **Manual:** Paste the Cookie header from a platform.parallel.ai request.
  Manual mode does not fall back to browser sessions.
- **Off:** Cookies are disabled.

The provider makes one read-only request:

```text
POST https://platform.parallel.ai/api/rpc/billing/payments/refresh-balance
Cookie: <session cookies>
Origin: https://platform.parallel.ai
```

- The **Balance** row shows the account balance in USD (cents divided by 100).
- **Pending debit** appears only when the account reports a non-zero pending debit.
- The balance is informational: it has no quota percentage and no cost.
- The provider reports identity as "Browser session".
- A signed-out response (401, 403, a redirect, or an HTML page) moves to the next
  session; when every session is signed out, CodexBar reports an expired session.
- Malformed responses fail parsing without echoing the response body.
- Parallel's upstream plugin reads only Chrome. This port uses the shared browser
  order and the explicit browser selection in Preferences.

### fal

fal is disabled by default. In Settings → Providers → fal, enter an **API key**
with billing read access (the BILLING or READONLY preset), or set `FAL_KEY`. The
key is stored with the other API keys and is never written to logs.

The provider makes one read-only request:

```text
GET https://api.fal.ai/v1/account/billing?expand=credits
Authorization: Key <API key>
```

- The **Credit balance** row shows `credits.current_balance` in the reported
  currency (`credits.currency` when it is a three-letter uppercase code, otherwise
  USD). A numeric string such as `"12.50"` is accepted.
- The account name (`username`) is shown as the organization. The provider
  reports identity as "API key".
- The balance is informational: it has no quota percentage and no cost.
- 401 reports a rejected key; 403 reports that the key needs the
  `billing:usage:read` permission; other HTTP errors report their status. A
  successful response without a readable balance reports the balance as
  unavailable, and Refresh can retry it.
- Response bodies are never echoed in errors.

### FLORA

FLORA is disabled by default. In Settings → Providers → FLORA, enter an **API
key** (or set `FLORA_API_KEY`) and the **workspace ID** (`ws_…`, or set
`FLORA_WORKSPACE_ID`). API keys cannot list their workspaces, so the workspace ID
is required. The workspace ID is stored in the provider's workspace slot and is
validated before any request. The key needs billing permission on that workspace.

The provider makes one read-only request for the current UTC month, from the
first day through today:

```text
GET https://app.flora.ai/api/v1/workspaces/<ws_id>/usage?start_date=YYYY-MM-01&end_date=YYYY-MM-DD
Authorization: Bearer <API key>
```

Rows in the **Workspace balance** section:

- **Available** shows `available_cost`. The secondary text is "Workspace pool"
  for a workspace balance and "Your allowance" for a per-caller balance.
- **Member limit** appears only for a per-caller balance with a finite
  `member_limit.limit_cost`. The secondary text is "Monthly" or "All time".
- **Spent this month** shows `spend.total_cost` when spend is reported, with a
  **Runs this month** row for a whole-number `classified_runs`. When spend is
  unavailable it shows "Unavailable" with the reason (no usage reporting, billing
  permission missing, or totals not yet computed).
- The currency is `currency` when it is a three-letter uppercase code, otherwise
  USD. The provider reports identity as "API key" and has no quota percentage.

Errors: 401 reports a rejected key; 403 reports that the key needs billing
permission on the workspace; 404 reports an unknown workspace ID; other HTTP
errors report their status. A response without a finite `available_cost` is
reported as unrecognized. Response bodies are never echoed in errors.

### Aerostack

Aerostack is disabled by default. In Settings → Providers → Aerostack, enter an
**account JWT** to monitor usage and allowance. Alternatively, set `AEROSTACK_TOKEN`.
The masked token is saved in CodexBar's local config file. This is the account
bearer token used by the billing API, not an Agent Endpoint key (`aek_…`).
Replace it when it expires; CodexBar does not sign in, refresh tokens, or import
browser credentials.

The provider makes one read-only request:

```text
GET https://api.aerostack.dev/api/billing/usage
Authorization: Bearer <account JWT>
```

- The **Monthly AI tokens** meter uses `usage.ai_tokens.used` and `usage.ai_tokens.limit`.
- Detail rows show tokens used, allowance, remaining tokens, any usage above the
  allowance, and the reported period (for example, `2026-03`). The account tier
  supplies the plan label.
- The account's returned limit is authoritative; CodexBar does not hardcode
  Free or paid quotas.
- Missing counters remain unknown. A zero or absent allowance has no percentage
  meter; any reported counters remain visible. Usage above the allowance is not
  a claim that the account is blocked: Aerostack documents a grace buffer.
- Invalid counters fail parsing without including the response body in the error.
- 401 reports authentication failure (expired or rejected JWT); 403, 429, and
  other errors report their status; response bodies are never echoed in errors.

### Cosmic AI

Cosmic AI is disabled by default. In Settings → Providers → Cosmic AI, enter a
**Personal Access Token** and the **Project ID** to monitor. Create a token in
[Cosmic Account Settings → API Tokens](https://app.cosmicjs.com/account/api-tokens).
Alternatively, set `COSMIC_TOKEN` and `COSMIC_PROJECT_ID`. The masked token and
Project ID are saved in CodexBar's local config file (`apiKey` and `workspaceID`
in the `cosmic` provider entry). Bucket read/write keys are not supported.

The provider uses the read-only endpoint underlying the official `cosmic billing usage` command:

```text
GET https://dapi.cosmicjs.com/v3/projects/usage?project_id=<Project ID>
Origin: https://app.cosmicjs.com
```

- The **Input tokens** and **Output tokens** meters pair `usage.ai.input_tokens`
  / `output_tokens` with `plan_info.ai_tokens.max_input` / `max_output`.
- Allowances can be decimal strings with `k` or `M` suffixes (as in the
  official CLI), explicit `unlimited` allowances, or absent.
- Detail rows show usage, allowance, remaining tokens, and any amount above the
  allowance separately for each dimension.
- Usage covers only the configured project; it is not summed across projects or
  workspaces.
- Missing counters remain unknown. A zero, absent, or nonnumeric allowance has
  no percentage meter; any reported counters remain visible. An explicit
  unlimited allowance stays labeled unlimited, and other unrecognized strings
  leave the allowance unknown.
- Invalid numeric counters fail parsing without including the response body in
  the error.
- 401, 403, and 429 report their status; response bodies are never echoed in
  errors.

## Listing what is enabled

```powershell
codexbar config providers
codexbar config enable -p cursor
codexbar config disable -p cursor
```

Desktop: Settings → Providers (sidebar reorder, per-provider credential UI).

## Status pages

Optional status polling (provider status pages) is available via CLI `--status` and Settings advanced toggles where wired. Mapping of Statuspage vs Google incidents is provider metadata in code — see provider modules rather than upstream-only URLs if they disagree.

## Usage & Spend

Desktop tab id: `usageSpend`. The desktop and Overview consume one shared spend catalog. Codex and Claude local logs are first-class; routed OpenCodex usage enriches the matching Codex, OpenCode Go, Kimi, or DeepSeek subscription instead of appearing as a second fake provider. xAI and OpenRouter can publish exact provider-metered daily USD spend when their management credentials are configured, while Grok local sessions contribute tokens only (the scan covers exactly the requested number of local calendar days, including today). Missing spend sources remain unknown rather than becoming a false `$0`. Do not invent cross-currency totals.

### AWS Bedrock monitoring

AWS Bedrock is a Windows provider backed by signed Cost Explorer requests and optional CloudWatch activity. It is disabled by default, and monitoring requests can add charges to your AWS bill. AWS currently charges $0.01 per Cost Explorer API request; paginated monthly-spend reads can therefore use more than one billed request, while optional CloudWatch activity is billed under CloudWatch pricing.

The shared refresh interval controls automatic provider polling. `0` / Manual disables the recurring timer, but explicit refreshes and **Refresh when the menu opens** can still fetch Bedrock data. Disable Bedrock itself to stop its app refreshes.

`CODEXBAR_BEDROCK_BUDGET` changes only the displayed monthly progress. It does **not** cap AWS charges, stop polling, or enforce a billing limit.

Custom pricing overlays are exact-match overrides used only where the local spend contract has matching provider/model token evidence. Explicit zero rates mean free; omitted rate fields stay unknown. The Usage & Spend surface keeps provenance/coverage visible, preserves cost-only model rows when token coverage is partial, and can Copy JSON or save the same JSON contract through the native file picker.

### OpenCode, Codex quota, and local cost boundaries

OpenCode-held OpenAI/Codex OAuth can be reused for **remote Codex account quota** only when the Codex provider's `External OAuth sources` setting is explicitly enabled. Native Codex credentials still take precedence, an explicit `CODEX_HOME` stays isolated, and external credentials remain read-only. This does **not** import ordinary OpenCode sessions into Codex token or spend totals. OpenCode Go's local SQLite reader remains scoped to its own `opencode-go` assistant records; OpenAI API-platform usage is a separate provider. Its recorded local token counts (`tokens` on assistant messages and step-finish parts; step-finish parts win over their parent message) fill Usage & Spend token columns and per-model history; costs stay the recorded `cost` field, and rows with absent, malformed, negative, or overflowing tokens leave that window's totals incomplete instead of counting as zero.

Codex local cost prices **Priority (Fast) turns** at the Fast rate. A turn counts as Priority when `<CODEX_HOME>/logs_2.sqlite` (Codex's trace database) holds a `response.create` websocket request with `service_tier == "priority"` for that turn's id. The database is opened read-only, scanned incrementally with a persisted cursor in the cost cache, and only turn ids, model names, and timestamps are kept; row bodies contain prompts and are never stored or logged. A missing or unreadable database keeps Standard pricing, and turn evidence never crosses `CODEX_HOME` scopes. Models without a Fast lane stay Standard. Older cost caches rebuild once (Codex cache schema v4 records each row's turn id).

### OpenRouter keys

The **API key** field (or `OPENROUTER_API_KEY`) is required and accepts either a regular API key or a Management API key. A Management key entered there also enables account Activity on the official OpenRouter API. The separate **Management API key** field (or `OPENROUTER_MANAGEMENT_API_KEY`) is an optional additional key for account Activity, only needed to use a Management key different from the one in the required field. Filling only the Management field does not select an account for quota or balance; with no primary key, the provider reports the missing-key message instead.

### z.ai Coding Plan quotas

z.ai Coding Plans accept both `TOKENS_LIMIT` and `CREDIT_LIMIT` rows. The shortest known Coding Plan window becomes primary and the longest becomes secondary; `TIME_LIMIT` is the separate MCP lane. When absolute usage/remaining counts are available they determine the used percentage, otherwise the provider percentage is used, always clamped to 0–100%. This behavior is shared by the tray, provider detail, CLI, and other Windows surfaces. Empty or wholly unrecognized limits never fabricate a 0% window; they show a "Coding Plan usage: Unavailable" detail row that points to the Usage Dashboard, and mixed responses keep the recognized windows and add "Additional quota: Unavailable". Unknown string limit types are skipped, while malformed entries and unsupported envelopes fail with Usage Dashboard guidance.

### xKiro daily free tokens

xKiro reads `GET https://api.xkiro.com/v1/usage` with `XKIRO_API_KEY` (or the key saved in Preferences) as a bearer token. Only the `free_tokens` counters are used: the primary lane is `used_today / limit_per_day`, resetting at the next 00:00 UTC, and a limit of `0` reads as fully used. The paid `wallet` balance is never treated as headroom and is not shown. Counters that are missing stay missing rather than becoming zero, a payload with no usable counter is reported as unrecognized, and an explicit `plan: null` is labelled "Pay as you go". There are no token accounts. The wire shape follows upstream's documented example; live account behavior has not been verified.

Upstream's independent **WidgetKit** provider-widget configuration has no Windows analogue in this repository. Win-CodexBar has no WidgetKit extension; provider cards and tray entries are already independent Windows/Tauri surfaces.

### LiteLLM budgets

LiteLLM reads a virtual key's own budgets from the proxy's management routes. Set the base URL and key in Settings → Providers → LiteLLM, or use `LITELLM_BASE_URL` and `LITELLM_API_KEY`. A trailing `/v1` is dropped. The base URL must use HTTPS unless it names `localhost`, a `.local` host, or a loopback, RFC 1918, link-local or IPv6 unique-local address, and it must not embed credentials, because the key is sent as a bearer token.

The provider calls `GET /key/info`, then `GET /user/info?user_id=…` for a user-bound key or `GET /team/info?team_id=…` for a team-only key, and rejects a response whose user or team ID differs from the key's. The personal budget fills the primary lane and the key's matching team budget fills the secondary lane. When only one budget exists, it takes the primary lane under its own label, and a key without any budget shows "No budget set". Amounts such as `$25.00 / $100.00` are detail lines, shown apart from a real reset date. The Automatic tray and float bar metric shows the team budget unless a budget is exhausted. Spend without a budget stays visible as API spend, and no pace is derived from budget resets.

## Upstream doc warning

Upstream `docs/providers.md` is a large auto-strategy matrix (60+ providers) for the macOS app. Use it as **inspiration** when porting a provider. For runtime truth on Windows:

1. `rust/src/core/provider.rs` (`ProviderId`)
2. `rust/src/providers/<id>/`
3. `codexbar usage -p <id> -v` / desktop provider detail errors

## Related

- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [CLI.md](./CLI.md)
- [CONFIGURATION.md](./CONFIGURATION.md)
- [COOKIES.md](./COOKIES.md)
