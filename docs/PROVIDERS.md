# Providers

This page explains how a provider is added and fetched in Win-CodexBar, how its usage windows and pace work, and what is special about individual providers. The upstream `docs/providers.md` describes the macOS app. Its ids and fallback order differ from this port, so check the code before you copy anything from it.

## Add a provider

1. Add a `ProviderId` variant in `rust/src/core/provider/id.rs`, and its metadata row in `rust/src/core/provider/spec.rs`: `cli_name`, `display_name`, `from_cli_name` and, if it uses cookies, the cookie domain.
2. Implement the `Provider` trait in `rust/src/providers/<name>/`.
3. Add a match arm in `instantiate` in `rust/src/core/provider_factory.rs`.
4. Add a match arm in `pace_capability` in the same file. Map the provider to its own `PACE_CAPABILITY`, or to `ProviderPaceCapability::UNSUPPORTED`. See [Pace](#pace).

Both matches are exhaustive, so a missing arm doesn't compile. A test checks that every id can be built.

`ProviderId` lives in `rust/src/core/provider/id.rs`. Its names, aliases, brand color and metadata are one row in `rust/src/core/provider/spec.rs`. The factory match is **exhaustive** (a missing arm is a compile error). Tests make sure every id instantiates.

Rules:

- `instantiate_provider` is the only provider factory. The desktop shell and the CLI both use it. Don't write another one.
- Keep parsing, auth and display choices inside the provider module. Shared code has no `if provider == X` branches. When shared code needs a provider-specific answer, add a typed field or a trait method with a default, as `menu_bar_pace_lane` and `reset_regenerates` below do.
- Keep identity, plan and email per provider. Never show one provider's account data on another provider's card or page.

`ProviderId` has 109 variants. A fresh install enables only Claude and Codex. Kimi K2 and CrossModel are deprecated and stay hidden unless they are already enabled.

## Fetch sources

| Source | Meaning |
| --- | --- |
| `auto` | The provider's own fallback order |
| `web` | Browser cookies or a pasted cookie against the provider's web dashboard |
| `cli` | A local CLI, PTY or RPC helper |
| `oauth` | An OAuth flow, where the provider supports it |

Pick one for a single CLI run with `codexbar usage --source <source>`. Most providers look for credentials in this order: the value saved in Settings, then the credential store, then environment variables. Each module sets its own order.

## Usage windows

A provider returns its usage as rate windows (`RateWindow` in `rust/src/core/rate_window.rs`). The tray panel draws one metric row per window, with a bar, the reset time and, where it applies, a pace line.

- `window_minutes` is the window length: 300 for a five-hour session, 10080 for a week, 43200 as the 30-day sentinel for a monthly window. A window with no length gets no tray pace tint and, for some rows, no pace line.
- `reset_regenerates` makes the card say "Regenerates in ..." instead of "Resets in ...", and hides the pace line for that window. Synthetic sets it on its regenerating lanes.
- `hidden_on_card` keeps the window for the tray icon and float bar, but the card draws no row for it. LithosAI uses it for its balance.
- `card_title_key` on `CostSnapshot` names the cost block. ClawRouter uses "Monthly budget" when the policy has a limit and "ClawRouter spend" when it doesn't. LithosAI uses "Credits".

These three fields cross to the frontend only when a provider sets them. The CLI JSON doesn't change.

`Provider::automatic_metric_absent_core_window_prefix()` names extra windows that stand in when a snapshot has no core quota lane (an informational primary and no secondary, model or tertiary lane). Automatic, and Session or Weekly when their lane is missing, then show the most constrained matching window instead of the placeholder. Claude returns `claude-weekly-scoped-`, so an account with only model-scoped weekly limits shows its scoped quota (upstream #4126). Its Daily Routines window and its spend cap never stand in.

Claude reads `limits[]` entry by entry for both OAuth and Web (upstream #4614). A malformed entry or a field of the wrong type does not hide its valid model-scoped siblings, a non-object entry is skipped, and a JSON Boolean is never a percentage.

A successful Claude Web answer with no quota window and no spend (Claude Free returns null `five_hour` and `seven_day`) shows the informational session row `Limits not available` instead of `No active 5h session` (upstream #4687). A prepaid balance or any measured lane keeps the existing presentation.

Automatic ranks its candidate windows by depletion risk (`usage_metric.rs`, upstream #4491): an exhausted window first, then a window whose current pace runs out before its reset (the soonest first), then the highest used percentage. Equal ranks keep the earlier lane. The forecast needs authoritative pace (estimates never forecast), a window with its own length and reset, and at least 3% expected use, so a fresh window doesn't win on a noisy rate. The highest-usage tray icon ranks each provider's selected metric the same way; the forecast counts only when that provider's metric is Automatic. `Provider::automatic_metric_follows_lane_order()` opts out of the ranking for ordered tiers: the first exhausted core lane wins, otherwise the first reported lane. Gemini (Pro before Flash) and Kiro (plan credits before bonus credits) return `true`.

`ProviderFetchResult::with_estimated_quota()` marks quota percentages that a provider computed locally instead of reading from the provider (upstream #4423). The bridge sends `quotaEstimated: true` only for such snapshots, and the tray then puts a `~` before the percentage in the percent icon, the tooltip and the provider status label. OpenCode Go sets it when its local SQLite estimate is shown; a console usage overlay clears it.

## Pace

Pace compares how much of a window you have used with how much of its time has passed. The card shows it as a line such as "10% in deficit · Runs out in 9d 8h". The shell scores it into seven stages, from `far_behind` to `far_ahead` (`commands/bridge/pace.rs`).

- Session and weekly windows get pace from their length and their reset time.
- **Work days** in Settings > Menu limits weekly pace to working days. When it is set from 2 to 6, a 10080-minute window counts only Monday up to that weekday as elapsed time. Day boundaries are local midnight. Automatic, 1 and 7 use plain elapsed time.
- The card hides the pace line at 100% used. It also hides it unless at least 3% use is expected or the quota runs out now.

### Reset-window pace for monthly and billing windows

Some providers refill on a billing date instead of on a fixed session or week. Those providers opt in to reset-window pace. Each one declares a `PACE_CAPABILITY` constant in its own module (`rust/src/core/pace_capability.rs` defines the type). The rule says which windows get pace and whether a 30-day sentinel becomes the real calendar month that ends at the reset, so a February cycle scores as 28 days.

| Provider | Windows that get pace | Uses the calendar month |
| --- | --- | --- |
| Copilot | any window with a reset date | when the window has no length |
| Cursor | any window with a length | no |
| Kimi | exactly 10080 minutes | no |
| Codex | a window reported as exactly 43200 minutes (`limit_window_seconds: 2592000`, seen on Free plan responses) | no, the reported 30 days are kept |
| Grok | a window labelled weekly whose reset falls inside it | no |
| Z.ai | the 43200-minute MCP window | yes |
| Amp | a window with a length whose reset text starts with "renews in " | no |
| Alibaba, Alibaba Token Plan, Command Code, Doubao, Kiro, MiMo, StepFun, Notion, Ollama, OpenCode Go | exactly 43200 minutes, or the calendar month ending at the reset | yes |

Every other provider is `UNSUPPORTED` and keeps only the session and weekly pace.

### Tray pace tint

With **Color Pace Indicator** on (`menu_bar_color_pace`, Settings > Menu Bar), a single-provider tray icon turns green when usage is behind pace and red when it is ahead. Stacked icons keep the usage color.

The tint is scored on one window:

- `Provider::menu_bar_pace_lane()` can name it. Codex returns its weekly lane and Abacus its primary lane.
- For every other provider (`None`), it is the window the icon draws for the selected metric.

The window must have a length, quota left and at least 3% expected use. Copilot's windows have no length, so Copilot never gets a tint.

## Devin manual authentication

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

Providers that read a web dashboard use cookies from a browser profile, or a cookie header you paste. Choose the browser for each provider in **Settings > Providers > (provider)**, then import. A pasted header is the fallback, and the only option under WSL. [COOKIES.md](./COOKIES.md#supported-browsers) lists the supported browsers.


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

### Command Code saved sessions

Command Code keeps several labeled subscription sessions under **Session tokens**
(upstream #4774). Paste a bare session token, a full Cookie header, or a cURL
capture; a bare token gets the production session cookie name. A cURL capture
must be a single line: the token command rejects multiline values with
"Token must be a single line".

- Saved sessions count only when the cookie source is **Manual**. In Auto the
  selected session uses browser billing even when an API key is configured, and
  a rejected session reports an error without trying a cached or browser
  account.
- The **API** usage source keeps saved sessions passive and uses the configured
  key. Cookie source **Automatic** or **Off** also keeps them passive.
- Unlike the Mac app, an empty Manual cookie source still falls back to browser
  cookies, because Manual is the Windows default cookie source.

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
- The prepaid USD balance (1 USD = 1,000,000,000 nanos) shows twice on the
  detailed card, as on the Mac. First comes a **Billing** section: Balance,
  Payment card, Account status, Today (UTC) and This month (UTC). Then comes a
  cost block titled **Credits** that reads, for example, "Balance: $42.37".
  There is no quota bar. The balance window is `hidden_on_card`, so it feeds
  the tray and float bar without drawing a card row.
- The compact Overview shows the Billing rows once and no Credits block.
- A negative balance has no cost block. It shows in the Balance row, for
  example "-$1.00".
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
  reserved (frozen) credits, and the cycle reset (cycle ends are China
  Standard Time; the listings share a 5-second budget and a failed listing
  only drops the reset).
- The reset follows the paid subscription cycle: the earliest future cycle
  end in the paid package listing. The free listing is queried only when the
  paid listing has no future end, so a trial grant that closes with the
  calendar month cannot shorten a paid cycle. Reward and purchased credits
  that end earlier are not reflected, so the balance can drop before the
  reset.
- The icon is a monochrome version of the mascot from WorkBuddy's
  [official logo](https://assets.workbuddy.cn/web/workbuddy/a5b2fb510f0e7b156f19d072fae0cd12ae1f2f19/assets/logo.svg),
  in the app and in the `codexbar serve` dashboard. The `#0DC8A6` accent is
  unchanged.
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
- Regenerating lanes keep their length (300 and 10080 minutes) and set
  `reset_regenerates`. The card counts them down as "Regenerates in 45m"
  instead of "Resets in", always as a relative time, and shows no pace line
  for them. Plain lanes keep "Resets in". The tray pace tint still scores a
  regenerating lane.
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
  `account.paygo_limit`), appear in a separate detail section. A null
  pay-as-you-go limit leaves the cap unspecified: the section shows only the
  reported usage, with no Limit or Remaining row.
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
  first day of the next month (UTC). The cost block is titled "Monthly budget"
  and shows this month's spend against the limit. Without a positive limit it
  is titled "ClawRouter spend". Both titles stay in English in every language,
  as on the Mac.
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

### Future AGI

Future AGI is disabled by default. It needs both an **API key** and a **secret
key**. In Settings → Providers → Future AGI, enter `API_KEY:SECRET_KEY` (split at
the first colon), or enter only the API key and set `FI_SECRET_KEY`. Without a
saved key the provider reads `FI_API_KEY` and `FI_SECRET_KEY`. A half saved in
Settings overrides its environment variable. Surrounding whitespace and one pair
of wrapping quotes are removed from each half. Upstream has two separate fields;
Win-CodexBar has one API key slot per provider, so both halves share it. Neither
key is written to logs, and a missing half fails before any request.

The provider makes one read-only request:

```text
GET https://api.futureagi.com/usage/v2/usage-overview/
X-Api-Key: <API key>
X-Secret-Key: <secret key>
```

- The response must have `status: true` and a `result` object with 1 to 16
  `dimensions`, a `billing_period_start` before `billing_period_end` (dates as
  `YYYY-MM-DD`, read as UTC midnight, or ISO 8601 timestamps with an offset) and
  a `total_estimated_cost`.
- Each dimension needs a unique `key`, a `display_name` and `display_unit` (at
  most 120 characters), and `current_usage` / `free_allowance` as non-negative
  JSON numbers. Numeric strings are rejected so malformed usage never reads as
  zero.
- The **Used / free allowance** section has one row per dimension:
  `used / allowance unit`, for example `500 / 2,000 credits`.
- The **Billing period** section shows **Estimated usage cost** in USD.
- The primary window is `ai_credits` and the secondary window is
  `gateway_requests`, each as usage against a positive free allowance (clamped
  to 100%), spanning the billing period and resetting at its end. A meter with a
  zero allowance keeps its row but has no bar.
- `plan_display_name` is shown as the plan. There is no cost summary.
- 401 and 403 report that authentication is required; 429 reports that usage
  requests are rate limited; other non-200 responses report
  `Future AGI returned HTTP <status>.` Anything else unexpected reports
  `Future AGI returned an unrecognized usage overview.` Response bodies are
  never echoed in errors.

### Munito

Munito is disabled by default. In Settings → Providers → Munito, enter an
**admin API key** with the "Read account data" (Admin API) permission, or set
`MUNITO_API_KEY` (`MUNITO_ADMIN_KEY` is also read). Surrounding whitespace and
one pair of wrapping quotes are removed. Create keys at
[console.munito.ai/keys](https://console.munito.ai/keys). The key is stored with
the other API keys and is never written to logs.

The provider makes two read-only requests at the same time:

```text
GET https://api.munito.ai/admin/v1/limits
GET https://api.munito.ai/admin/v1/organization
Authorization: Bearer <admin API key>
```

The organization read is best effort. When it fails or returns something other
than a JSON object, the spend limits still show and only the organization rows
and name are missing.

- `periods` entries with `period` `day` and `month` become the **Today** and
  **This month** rows in the **Spend limits** section. Other periods are
  ignored. A row shows `$spent of $limit` with a progress bar when `limit_usd`
  is positive, otherwise only the spend.
- **Today organization limit** / **Today account limit** (and the monthly
  equivalents) appear when that cap is positive and differs from `limit_usd`.
  **Today stopped until** / **This month stopped until** show `stopped_until`
  in UTC; the daily and monthly stops stay separate.
- **Usage tier** comes from the limits response, or else from the organization.
- The **Organization** section shows **Status** and **Data region**. The
  organization `name` is shown as the account organization. The provider reports
  identity as "Admin API key".
- The primary window is today's spend against a positive daily limit (one-day
  window); the secondary window is this month's spend against a positive monthly
  limit. Cost uses this month's spend and limit, or today's when no month period
  is reported. With no periods, the card shows only the identity and the rows.
- 401 reports that authentication is required; 403 reports that the key needs
  the "Read account data" permission; 429 reports that API requests are rate
  limited; other HTTP errors report `Munito returned HTTP <status>.` Amounts
  must be non-negative numbers or numeric strings, and `stopped_until` must be
  an ISO 8601 timestamp; anything else fails parsing. Response bodies are never
  echoed in errors.

### Soniox

Soniox is disabled by default. In Settings → Providers → Soniox, enter an
**organization API key** with the "Balance and spend" and "Usage and limits"
permissions, or set `SONIOX_ORG_API_KEY`. Project API keys cannot read
organization billing. Create keys in the
[organization API guide](https://soniox.com/docs/guides/organization-api#organization-api-keys).
The key is stored with the other API keys and is never written to logs.

For proof runs and fake servers, `SONIOX_BASE_URL` replaces the organization API
base (default `https://api.soniox.com/v1/organization`). It accepts `https://`,
or `http://` only for a loopback host (`127.0.0.1`, `localhost`, `[::1]`), with
no credentials, query, or fragment. The path must end in `/v1/organization`.
Any other value, including a blank one, is ignored with a warning.

The provider makes two read-only requests in order. Refresh never submits
transcription or synthesis work:

```text
GET https://api.soniox.com/v1/organization/balance
GET https://api.soniox.com/v1/organization/usage/monthly
Authorization: Bearer <organization API key>
```

- The **Spend** section shows **Monthly budget** as `$used of $budget` with a
  progress bar when the budget is positive. The budget meter itself is hidden on
  the card and only feeds the tray and float-bar percentages. Upstream hides it
  from the card by default; Windows has no default-hidden usage item, so the
  card shows the same numbers as this Spend detail row instead.
- The **Billing** section shows **Prepaid** or **Invoice billing** (value `USD`).
  With a zero budget it also shows **Monthly budget** as `$0.00`, and no progress
  bar or percentage.
- Cost is this month's spend against the budget, reset at the start of the next
  UTC month. Prepaid accounts also report the remaining balance. Invoice billing
  accounts omit the balance so a zero prepaid amount is not shown as exhausted
  credits. A negative prepaid balance is accepted but not shown.
- 401 reports that authentication is required. 403 reports that the key needs
  organization permissions. 429 reports rate limiting. Other HTTP errors report
  the status. Amounts must be plain decimal strings, `month` must be `YYYY-MM`,
  and `invoice_billing_enabled` must be a boolean; anything else fails parsing.
  Response bodies are never echoed in errors.

### Claude Platform

Claude Platform is disabled by default and shows the prepaid API credits of a
Claude Console (platform.claude.com) organization, including the monthly API
credit that Max plans include. It is separate from the Claude
provider: it never reads Claude's cookies, OAuth credentials, plan or email.
Sign in to platform.claude.com in your browser, and CodexBar uses that
session's `sessionKey` cookie.

- **Automatic:** A previously imported Cookie header is used on its own.
  Otherwise CodexBar tries each detected browser holding a platform.claude.com
  `sessionKey`. If the Console rejects a session (401 or 403), it moves to the
  next browser. No session means sign in to platform.claude.com.
- **Manual:** Paste the Cookie header from a platform.claude.com request. It
  must contain `sessionKey`. Manual mode does not fall back to browser sessions.
- **Off:** Cookies are disabled.

The provider makes up to three read-only requests:

```text
GET https://platform.claude.com/api/organizations
GET https://platform.claude.com/api/organizations/<uuid>/prepaid/credits
GET https://platform.claude.com/api/organizations/<uuid>/current_spend
```

- Only organizations whose `capabilities` include `api` are used; Claude.ai chat
  organizations are skipped. The first API organization whose billing the member
  can read wins (a 403 moves to the next one). An account without an API
  organization reports that, and every organization answering 403 reports that
  billing access was denied.
- The **API credit** lane is the soonest-expiring `subscription_api_credit` grant
  that still has credit. The **Purchased credit** lane is the soonest-expiring
  purchased grant that still has credit, falling back to the first purchased
  grant when all are spent. Each lane shows used percent and a detail such as
  `$124.75 of $200.00 left · Expires Oct 28 (UTC)`. Expiry dates are UTC calendar
  dates; the year is added when it is not the current year.
- Other unexpired grants appear in **Other credit grants** with their own
  progress. Expired grants (expiry at or before now) are dropped. A malformed
  expiry is treated as unknown.
- The credit balance shows in the card's Credits block as prepaid credits.
  Amounts use the reported currency and credit exponent (0 to 4, default 2).
- **Billing → This month (UTC)** shows month-to-date spend. Spend is optional:
  any failure shows "Spend: Unavailable" and keeps the balance.
- The provider reports identity as "Console session" and the Console
  organization name. It never reports an email.
- A missing, non-integer, or out-of-range balance fails parsing instead of
  showing $0. 429 reports rate limiting and 5xx reports the Console as
  unavailable. Response bodies are never echoed in errors.

### Trae CN

Trae CN is disabled by default and shows the credit allowance of a Trae CN
(trae.cn) account. Sign in to trae.cn in your browser, and CodexBar uses that
session.

- **Automatic:** Trae keeps its session cookies on the parent `trae.cn` domain,
  so CodexBar reads the `trae.cn` cookies and sends only the ones a browser
  would send to `api.trae.cn`. It tries each detected browser holding such a
  session. If Trae rejects a session (401, 403 or a redirect), it moves to the
  next browser. No session means sign in to trae.cn.
- **Manual:** Paste the Cookie header from an api.trae.cn request. Manual mode
  does not fall back to browser sessions.
- **Off:** Cookies are disabled.

The provider makes two read-only POST requests per session and never makes a
billable call:

```text
POST https://api.trae.cn/cloudide/api/v3/common/GetUserToken
POST https://api.trae.cn/trae/api/v2/pay/user_current_entitlement_list
```

- The token from `GetUserToken` lives only for one refresh and is sent only to
  api.trae.cn as a `Cloud-IDE-JWT` authorization header.
- The gauge is consumed over total credits. A zero allowance shows the details
  without a gauge. Used, Total and Remaining appear in the **Credits** section.
- The gauge is labeled **Usage** and the Used / Total / Remaining block
  **Credits**, so the two never share a name. The visibility IDs are
  unchanged, so saved hidden choices keep working.
- The plan name comes from the active plan entitlement and is shown as the
  login method. Pack expiry dates do not set a reset: the provider infers no
  renewal or reset date.
- Malformed responses (no `is_credits_billing`, non-numeric or negative amounts,
  or no token) fail parsing instead of showing zero usage. 429 reports rate
  limiting, 408 and 5xx report the service as unavailable, and response bodies
  are never echoed in errors.
- For proof runs and fake servers, `TRAE_CN_BASE_URL` replaces the API origin
  (default `https://api.trae.cn`). It accepts `https://`, or `http://` only for a
  loopback host (`127.0.0.1`, `localhost`, `[::1]`), with no credentials, query,
  or fragment. Any other value is ignored with a warning.

### Chatbox AI

Chatbox AI is disabled by default. In Settings → Providers → Chatbox AI, paste
your Chatbox **license key** into the API key field, or set
`CHATBOX_LICENSE_KEY`; the saved key wins. Signing in to the Chatbox app does
not configure this provider, and CodexBar never reads the Chatbox desktop
profile or activates a license. Keep the key private: it can authorize AI
requests as well as quota reads. It is never written to logs or errors.

The provider makes one read-only request per refresh and never makes a
billable call:

```text
GET https://api.chatboxai.app/api/license/detail/realtime
Authorization: <license key>
Cache-Control: no-store
```

The key is sent as is, without a `Bearer` prefix, as Chatbox's public client
does. A key longer than 4096 characters or with spaces or control characters is
rejected before any request.

- The **Remaining** section shows `remaining / total` counts: **Plan** (the
  `plan` item of `unified_token_usage_details`), **Quota** (the combined
  `unified_token_usage` / `unified_token_limit`, only when there is no plan
  pool), **Credits** (`aggregated_reward_details`, or the unexpired non-plan
  detail items), **Top-up credits** (`expansion_pack_*`) and **Image
  Generation** (`image_*`). These are compute points and image counts, not
  money. Empty pools are hidden; exhausted pools stay. If every allowance is
  zero, the section shows `Quota 0 / 0`.
- The **Plan** meter is the primary window, only for a positive plan whose
  status is absent or `active` and that has not expired. Its reset is
  `token_next_refresh_time` when that is in the future and before the plan
  expiry. Other pools never inherit it. **Top-up credits** with a positive
  limit is the secondary window, with no reset.
- The license `name` is the plan label. `token_expire_time` (or `expires_at`)
  is the subscription expiry.
- 401 reports an invalid key and asks for a new one. 403 reports denied access,
  429 rate limiting, and other statuses a failed request, each with its HTTP
  status. A body that is not the expected JSON envelope, an `error` member,
  negative, fractional or non-numeric counters, or a duplicate plan item fail
  parsing. Response bodies are never echoed in errors.
- For proof runs and fake servers, `CHATBOX_BASE_URL` replaces the API origin
  (default `https://api.chatboxai.app`). It accepts `https://`, or `http://`
  only for a loopback host (`127.0.0.1`, `localhost`, `[::1]`), with no
  credentials, query, or fragment. Any other value is ignored with a warning.

### abliteration.ai

abliteration.ai is disabled by default. In Settings → Providers →
abliteration.ai, enter your **API key**, or set `ABLITERATION_API_KEY`; the
saved key wins. The key is stored with the other API keys and is never written
to logs or errors.

The provider makes one non-billable request per refresh. It never sends
inference requests, imports browser cookies, or starts a login flow:

```text
GET https://api.abliteration.ai/v1/credits
Authorization: Bearer <API key>
```

The response contract is `{"data":{"total_credits":50.0,"total_usage":0.010637}}`;
both counters are USD.

- The **Credits** meter is the primary window: used / total dollars (for
  example `$0.01 / $50.00`), capped at 100% when overspent. It has no reset
  date and no pace, because the counters are cumulative.
- The **Credits** section shows **Balance** (total minus usage, floored at
  zero), **Credits used** and **Total credits**. Overspent accounts keep the
  reported spend. With zero total credits the dollar rows stay and the meter is
  hidden, so no percentage is invented. The Mac plugin labels the total row
  **Credits**; the Windows port says **Total credits** so it does not share a
  name with the meter in Settings → Visible usage items.
- Card bars use the label color, as on the Mac (`progressColorStyle: .label`).
- Missing, negative, non-numeric or non-finite counters fail parsing. 401 and
  403 ask you to check the API key in Settings, 429 reports rate limiting, and
  any other status reports a failed request. Response bodies are never echoed
  in errors.
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

### Vercel AI Gateway

By default the card shows the team's AI Gateway balance and lifetime spend. It
reads `GET https://ai-gateway.vercel.sh/v1/credits` with `AI_GATEWAY_API_KEY`
(or the key saved in Settings) and reports no quota percentage.

The personal budget is opt-in and off until you add a Vercel access token. In
Settings → Providers → Vercel, enter the **Access token** and the **Team ID**, or
set `VERCEL_ACCESS_TOKEN` and `VERCEL_TEAM_ID`. The token is stored with the
provider's secret settings, like the OpenRouter management key, is redacted from
config dumps and is never logged. The Team ID is stored in the provider's
workspace slot and is validated before any request. Clearing the token returns
the card to team credits.

With a token, the provider makes these read-only requests to `api.vercel.com`:

```text
GET https://api.vercel.com/v2/user
GET https://api.vercel.com/ai-gateway/budgets/list?teamId=<team>&scopeType=user&limit=200&cursor=<next>
Authorization: Bearer <access token>
```

- The budget list is followed by its cursor for at most 20 pages. A repeated
  cursor or a page limit reached before the list ends fails the refresh.
- A budget applies to the token's user when its scope ID is the user's ID or
  the `usr_`-prefixed form. A custom budget wins over an inherited default. Two
  budgets that both apply are ambiguous and fail the refresh.
- With no applicable budget, the card shows "No personal budget configured",
  not zero spend.
- The primary window shows spend as a share of the limit. Rows under
  **Personal budget** show Spend, Limit and Remaining (never below zero).
- Daily, weekly and monthly budgets reset at UTC boundaries: midnight UTC each
  day, Monday 00:00 UTC each week, and the first day of the month at 00:00 UTC.
  A `none` budget is cumulative and has no reset.

The access token is sent only to `api.vercel.com`. The AI Gateway key is sent
only to `ai-gateway.vercel.sh`.

Status handling differs by request:

- `/v2/user`: 401 means Vercel did not accept the token, so the card asks you
  to check or replace it. 403 is returned both for a bogus token and for a
  scoped token without the `user` permission, so the card asks you to check the
  token's scope or replace it.
- `/ai-gateway/budgets/list`: 401 asks you to check the token. 403 means the
  token's user needs a non-Contributor role on the team to read budgets.
- 429 and 5xx report their status on either request.

A token without a Team ID fails with a message that asks for one. Response
bodies are never echoed in errors.

## Grok

Grok reads its usage from the Grok CLI login in `~/.grok/auth.json`, a saved
account row, a token account, or a browser session. A bearer login tries these
sources in order and stops at the first one that answers:

1. Credits proxy: `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits`
   with the login's bearer token. If the answer has no quota percent, grok.com
   gRPC-web may supply one within 6 seconds.
2. grok.com gRPC-web billing (`POST .../GetGrokCreditsConfig`), used when the
   proxy fails.
3. Grok CLI ACP billing, a fallback that runs only when 1 and 2 both fail.

The ACP fallback (upstream #4595) runs only for the ambient Grok CLI login. It
also runs when that login is missing (no `~/.grok/auth.json` or no Cli entry) or
expired, since the installed CLI may hold its own valid session. In that case
there is no CodexBar token, so the answer has no account identity and no
reset-credit lookup. If ACP also fails, the original login error is shown, not
an ACP error. It never runs for an isolated token account, a saved account row,
or an OAuth principal, so it can never read a different account's usage:

- The installed `grok agent stdio` is found the same way as the login flow: the
  native `~/.grok/bin/grok.exe` first, then `PATH`. It is started with no console
  window and its stderr discarded. Requests are newline-delimited JSON-RPC 2.0 on
  stdin and stdout.
- While waiting for a reply, the client skips non-JSON lines, notifications, and
  any message with a `method`, even when its `id` matches. Those are requests
  from the server, not replies.
- `initialize` is sent first with `protocolVersion` `"1"` and filesystem and
  terminal capabilities off, with a 4-second budget. Then `_x.ai/billing` is
  sent with empty params, with a 3-second budget. The method name is written
  literally, without a `\/` escape.
- The child is killed after every call, including on timeout. It is never left
  running.
- A current result has the nested `config` credits shape, read by the same
  parser as the credits proxy. A top-level `subscription_tier` names the plan
  and overrides `config.subscriptionTier`. The older top-level shape with
  `monthlyLimit` and `usage.totalUsed` is still read.
- A prepaid balance counts as an answer without a quota percent. A result with
  neither a percent nor a balance is unavailable.
- When the ACP answer from a bearer login has no quota percent, grok.com gets
  the same bounded 6-second retry as the credits proxy (upstream #4679). Only a
  wire-published percent or a validated zero is adopted, and the ACP period,
  plan and balance stay. Expired or slow enrichment keeps usage unknown. A
  missing or expired login has no bearer to send, so this retry does not run.
- A JSON-RPC `-32601` error means this CLI does not provide billing over ACP.
  An error that says authentication is required, or mentions `grok login`, is
  an auth failure. In both cases the bearer path's original error is shown, so
  auto mode continues to its next source as before.
- Not ported: upstream's team-principal diagnostic, which turns a `-32601`
  answer for a team login into an identity-only row with an unsupported-team
  usage message. On Windows that case shows the original error.

## Antigravity Gemini quota summary

The Antigravity quota summary picks one Gemini row for the primary and secondary windows (upstream #4659).

- The ordinary Gemini buckets win: `gemini-5h` (or a session alias such as `gemini-session`) and `gemini-weekly`. Their IDs are matched after lowercasing, with `_` read as `-` and a trailing ` limit` ignored. Display labels never decide this.
- A model-specific Gemini allowance, such as `gemini-2.5-pro-5h`, represents Gemini only when the ordinary bucket of that cadence is absent, unknown or disabled.
- Every bucket stays visible as an extra window with its own usage and reset.
- A group named like "Gemini" with no digit is shown as **Gemini** (for example **Gemini weekly**). A versioned Gemini group keeps its own name from the usage response, so a separate Gemini 4 Argon group is shown as **Gemini 4 Argon weekly**, alongside the ordinary **Gemini weekly** quota (upstream #4585).

## Window lengths that match the Mac

These providers set window lengths so their rows get the same pace as in the Mac app, and the tray tint can score them. Where the stored length differs from the Mac's, the row says so:

| Provider | Window | Length |
| --- | --- | --- |
| Codebuff | Weekly lane | 10080 minutes. The Credits lane has no length. |
| MiniMax | Services rows from the coding-plan page | From the row's window type, for example 300 for 5 hours and 10080 for Weekly |
| MiniMax | Remains weekly rows | Always 10080 minutes, even without a start and end |
| Synthetic | Regenerating five-hour and weekly lanes | 300 and 10080 minutes, from the payload |
| Grok | Credits proxy period | Kept after the period ends. An ended period has no pace. |
| Z.ai | `TIME_LIMIT` windows | From the unit: 1 = 1440, 3 = 60, 5 = 1, 6 = 10080 minutes per unit. The monthly MCP marker keeps the 30-day sentinel. |
| Notion | Billing lane | Labelled "Monthly". Other monthly lanes keep the "30-Day" label. |
| Kiro | Plan credits lane | The real calendar month ending at the reset (CLI `resets on MM/DD` or the API `nextDateReset`), 28 to 31 days. The Mac stores the 43200-minute sentinel and resolves it to the same month, so the pace is identical. No reset means no length and no pace; an expired reset gives no pace. Bonus and overage lanes get no pace. |

## See what is enabled

```powershell
codexbar config providers
codexbar config enable -p cursor
codexbar config disable -p cursor
```

In the app, use **Settings > Providers**. The sidebar can reorder and toggle providers, and each provider page holds its credentials. See [CONFIGURATION.md](./CONFIGURATION.md#provider-list-in-the-sidebar).

## Status pages

The app polls provider status pages while **Settings > General > Check provider status** (`status_checks_enabled`, on by default) is on. A full provider refresh starts a status poll for the enabled providers that have a status source. The auto-resume probe (Windows-only, 60-second exact-session watcher) does not poll status. Unforced refreshes skip a provider whose last poll started less than 30 seconds ago; forced refreshes (manual, menu open, settings toggle) always poll. One fetch times out after 10 seconds and the whole poll after 15 seconds. A failed fetch keeps the last good status.

Where the status shows:

- **Settings sidebar.** For a provider with a status source, a service dot replaces the fetch-status dot: green for operational, yellow for a partial outage, orange for a major outage, red for a critical issue, gray for maintenance or unknown (also before the first poll). The tooltip reads "Provider service status: ..." with the label.
- **Tray panel.** While the selected provider has an incident, the menu shows one extra line under the provider actions: the feed's description (or the status label) and when the feed was updated.
- **Tray icon tooltip.** Each enabled provider with an incident adds a `Provider: label` line under the tooltip title.

The status source per provider is set by `status_source` in `rust/src/status/mod.rs`:

| Source | Providers |
| --- | --- |
| Statuspage (`/api/v2/status.json`) | Claude and Claude Platform (status.claude.com), Codex and OpenAI API (status.openai.com), Copilot (githubstatus.com), Cursor, Factory, Augment, Hugging Face, ZoomMate (zoomstatus.com) |
| Google Workspace incident feed | Gemini, Antigravity |

Other providers have no status source, so they keep the fetch-status dot and never add an incident line.

`codexbar usage --status` fetches the same sources from the CLI and adds them to the output. In JSON, `status.level` holds: `operational`, `degraded` (raw `minor`), `major` (raw `major` or `critical`), or `unknown` (raw `maintenance` or `unknown`). When this table and the code disagree, the code wins.

## Usage & Spend

The Settings pane id is `usageSpend`. The pane and the Overview read one shared spend catalog.

- Codex and Claude local logs are first-class sources.
- Routed OpenCodex usage is added to the matching Codex, OpenCode Go, Kimi or DeepSeek subscription. It doesn't appear as a separate provider.
- xAI and OpenRouter can report exact daily USD spend from the provider when their management credentials are set.
- Grok local sessions add tokens only. The scan covers exactly the requested number of local calendar days, including today.
- A missing spend source stays unknown. It never shows as `$0`.
- Totals are never converted across currencies.

### Claude local cost history

Claude local cost reads Claude transcript `.jsonl` files and prices assistant responses.

- The default history, with `CLAUDE_CONFIG_DIR` unset or blank and no mapped token account, also reads Claude Desktop on Windows: `%APPDATA%\Claude` and the Microsoft Store package stores. It takes the project stores and Cowork (local agent mode) `audit.jsonl` files.
- Cowork rows use the CLI transcript parser. A response present in both is counted once. `result` rows, which hold session totals, are never counted.
- A mapped token account or a set `CLAUDE_CONFIG_DIR` leaves Claude Desktop out. Only the default history includes it.
- Desktop discovery stops at 16,384 directory entries or 5 seconds. If it stops early, the Claude scan counts an unreadable root, so cost stays unavailable rather than showing a partial total.
- Symbolic links are not followed during Desktop discovery.
- Windows keeps no on-disk Claude cost cache. Each scan rereads the transcripts in the window.

### AWS Bedrock monitoring

AWS Bedrock uses signed Cost Explorer requests and, optionally, CloudWatch activity. It is disabled by default.

- Monitoring can add charges to your AWS bill. AWS charges $0.01 per Cost Explorer API request, and a paginated monthly-spend read can take more than one request. CloudWatch activity is billed at CloudWatch prices.
- The shared refresh interval controls automatic polling. Manual (`0`) stops the timer, but an explicit refresh and **Refresh when the menu opens** still fetch Bedrock data. Disable Bedrock to stop its refreshes.
- `CODEXBAR_BEDROCK_BUDGET` only changes the monthly progress shown. It doesn't cap AWS charges, stop polling or enforce a limit.

Custom pricing overlays override prices only where the local spend data has matching provider and model token evidence.

- An explicit zero rate means free. A rate you leave out stays unknown.
- Usage & Spend keeps provenance and coverage visible, and keeps cost-only model rows when token coverage is partial.
- **Copy JSON** and **Save JSON** export the same JSON contract.

### OpenCode, Codex quota and local cost

OpenCode can hold an OpenAI or Codex OAuth login. Win-CodexBar reuses it for **remote Codex account quota** only when the Codex provider's **External OAuth sources** setting is on.

- Native Codex credentials still come first.
- An explicit `CODEX_HOME` stays isolated.
- External credentials are read-only.
- Ordinary OpenCode sessions are never added to Codex token or spend totals.

### OpenCode web usage: Console and legacy sessions

OpenCode reads usage from `opencode.ai` cookies. Which API it uses depends on the session cookie (upstream #4131/#4139).

- A `__Host-console_session` cookie means a migrated Console workspace. Win-CodexBar reads `/console/api/orgs` to find the workspace, then `/console/api/go/status` for the 5-hour and weekly quota. Every call except the workspace lookup sends the `x-org-id` header.
- If the Go status is `null` or `{"access": null}`, Win-CodexBar shows spend only after `/console/api/orgs/current` reports `hasGoSubscription: false` and `/console/api/billing/status` reports `mode: "pay-as-you-go"`. The spend is `totalCostMicroCents / 1e8` from `/console/api/usage/summary?range=30d`. The card shows it as **Last 30 days** spend, with no limit and no percentage. A missing prepaid balance hides only the balance.
- A subscribed workspace without quota, or an invoiceable account, is an error unless the legacy session succeeds. It never shows as pay-as-you-go spend.
- A Console 401 means invalid credentials. Any other Console error is an API error, not an expired session.
- A cookie with only a legacy session (`auth` or `__Host-auth`) keeps the legacy `_server` path and its monthly pay-as-you-go billing fallback.
- When both cookies are present and the Console call fails, the legacy path is tried. If the legacy path also fails, the Console error is reported, unless the Console error was a 401.
- OpenCode reuses the OpenCode Go Console client (`providers/opencodego/console.rs`). The monthly Go meter appears only on OpenCode Go.
- For OpenCode Go, a manual cookie keeps the configured usage source (Auto, Web or Local) instead of forcing Web (upstream #4616). The cookie scopes only the web session. The source label shows the source that actually supplied the result, such as `web` or `local estimate`.

OpenCode Go's local SQLite reader reads only records attributed to `opencode-go`: legacy assistant messages and v2 `session_message` assistant and compaction requests. A v2 request is skipped only when a legacy copy with the same id has a usable timestamp and a non-negative finite cost, so migrated requests count once and an invalid legacy copy never hides a valid v2 request. V2-only databases (no `message` table, no `auth.json`) still read. OpenAI API-platform usage is a separate provider.

OpenCode Go reads server quota with the active OpenCode CLI login when no API key is configured (upstream #4771), so a signed-in CLI no longer falls back to the local estimate.

- Unscoped Auto keeps this order over local history: a configured API key, then a usable browser session, then the CLI login. A successful CLI read reports `local+cli`. Without local history, Auto tries the API key, then the browser session, then the CLI login, which reports `cli`. An estimate that already carries a browser account's Zen balance never borrows the CLI login.
- API mode (the `oauth` source) uses the configured key, else the CLI login, without importing cookies. A rejected configured key never falls back to the CLI login.
- Selected accounts, manual cookies, workspace overrides and Web mode never use the CLI login. Cookie source Off disables browser discovery, not the CLI login.
- OpenCode v2 keeps the login in the `credential` table of `opencode.db`. An active `opencode-go` row wins over the general `opencode` row, which counts only as an unexpired OAuth login. Two active rows for the same integration are ambiguous and fail. Once the credential table exists, a stale legacy `auth.json` is ignored, including after logout. Older installs use the `opencode-go` entry of type `api` in `auth.json`.
- Lookup uses the first OpenCode data directory that has `opencode.db` or `auth.json`: `%USERPROFILE%\.local\share\opencode`, then `%LOCALAPPDATA%\opencode`. An absolute `XDG_DATA_HOME` replaces them with `<XDG_DATA_HOME>\opencode`. `OPENCODE_DB` names the database file or gives an absolute path; `:memory:` disables the lookup.
- The credential is read-only and reread on every refresh. Win-CodexBar never refreshes tokens; renew an expired OAuth login through OpenCode. Read or decode failures report invalid credentials without database contents or tokens. In Auto they keep the marked local estimate.

For `codexbar cost --provider opencodego` (upstream #4629), token counts follow the same rule per row. A row with missing, malformed, negative or overflowing tokens is unknown, not zero, and any window or model containing one has no exact token total. A row that records a total but omits input or output keeps that exact total, while its input and output are unknown. The text report says `unknown` for the affected totals while still reporting cost. The JSON payload uses `null` for them and adds `unknownTokenRowCount` and `byModelTokens`. Per-model token counts sum only what rows recorded. The `spendContract` per-model rows still carry those usable partial counts; the desktop Usage & Spend view is unchanged.

- Token counts come from `tokens` on assistant messages and step-finish parts. A step-finish part wins over its parent message.
- They fill the Usage & Spend token columns and per-model history. Costs are the recorded `cost` field.
- A row with missing, malformed, negative or overflowing tokens marks that window's totals incomplete. It isn't counted as zero.

Codex local cost prices **Priority (Fast)** turns at the Fast rate.

- A turn is Priority when `<CODEX_HOME>/logs_2.sqlite`, Codex's trace database, has a `response.create` websocket request with `service_tier == "priority"` for that turn id.
- The database is opened read-only and scanned incrementally, with a cursor saved in the cost cache.
- Only turn ids, model names and timestamps are kept. Row bodies contain prompts and are never stored or logged.
- A missing or unreadable database keeps Standard pricing. Turn evidence never crosses `CODEX_HOME` scopes.
- Models without a Fast rate stay Standard.
- Older cost caches rebuild once. Codex cache schema v4 records each row's turn id.

### Cursor app session

Automatic mode first reads the signed-in Cursor app's `cursorAuth/accessToken` from `%APPDATA%\Cursor\User\globalStorage\state.vscdb` (read-only) and rebuilds the session cookie from it.

- Cursor 3.27 and later may store that token as a `cursor-safe-storage:v1:` envelope. CodexBar decrypts it with the DPAPI-protected key in `%APPDATA%\Cursor\Local State` (AES-256-GCM, as Chromium does on Windows). A plain JWT never reads that key.
- An unreadable, malformed or unknown-version envelope yields no app session, and Automatic mode falls through to browser cookies.

### Cursor unified personal spend

Cursor accounts on a unified membership with a user-level limit (`membershipType` starts with `unified_` and `limitType` is `user`) read monthly additional spend from `GET /api/dashboard/v2/usage-status?scope=user`, using the same session cookie.

- A valid, available response supplies the spend, the cap (cents converted to USD) and the reset time. Separate credit consumption is not added to the spend.
- The read has a five-second budget. If it fails, or the response is unavailable, malformed, or outside its period, the usage-summary values stay in place.
- A valid zero stays zero. Team budgets, legacy plans and the Grok Bot allowance keep their existing paths.

## API key notes

### Kimi Total usage breakdown

When Kimi reports the Kimi Code share of the monthly **Total usage** pool, the provider page adds a "Total usage" section with two rows: **Kimi Code** and **Other features** (the total minus the Code share, never below 0%). Both rows are details, not quota windows, because they draw on one limit.

- The Code share always comes from the source that supplied Total usage: `limit_month_code` next to the Code API `limit_month_total`, or `kimiCodeUsedRatio` next to the web membership pool. A Code API Total usage never borrows the web Code share.
- A missing, negative or malformed Code share leaves Total usage without a breakdown and keeps every quota window.
- A share above 0% but below 1% shows as "<1% used", as upstream does, even when it would round to 1%.

### OpenRouter keys

- The **API key** field (or `OPENROUTER_API_KEY`) is required. It takes a regular API key or a Management API key. A Management key there also turns on account Activity from the official OpenRouter API.
- The **Management API key** field (or `OPENROUTER_MANAGEMENT_API_KEY`) is optional. You need it only to use a different Management key for account Activity.
- A Management key alone doesn't select an account for quota or balance. Without the main key, the provider reports the missing-key message.

### z.ai Coding Plan quotas

- z.ai Coding Plans accept both `TOKENS_LIMIT` and `CREDIT_LIMIT` rows. The shortest known Coding Plan window is the primary lane and the longest is the secondary lane. `TIME_LIMIT` is the separate MCP lane.
- When absolute used and remaining counts exist, they set the used percentage. Otherwise the provider's percentage is used. Both are clamped to 0-100%. The tray, the provider page and the CLI all use this rule.
- Empty or unrecognized limits never invent a 0% window. They show a "Coding Plan usage: Unavailable" row that points to the Usage Dashboard. A mixed response keeps the windows it recognizes and adds "Additional quota: Unavailable".
- Unknown string limit types are skipped. Malformed entries and unsupported envelopes fail with Usage Dashboard guidance.

### xKiro daily free tokens

xKiro reads `GET https://api.xkiro.com/v1/usage` with `XKIRO_API_KEY`, or the key saved in Settings, as a bearer token.

- Only the `free_tokens` counters are used. The primary lane is `used_today / limit_per_day` and resets at the next 00:00 UTC. A limit of `0` reads as fully used.
- The paid `wallet` balance is never treated as headroom and isn't shown.
- Missing counters stay missing; they don't become zero. A payload with no usable counter is reported as unrecognized.
- An explicit `plan: null` is labelled "Pay as you go".
- There are no token accounts.
- The response shape follows upstream's documented example. It hasn't been checked against a live account.

### LiteLLM budgets

LiteLLM reads a virtual key's own budgets from the proxy's management routes. Set the base URL and key in **Settings > Providers > LiteLLM**, or with `LITELLM_BASE_URL` and `LITELLM_API_KEY`.

- A trailing `/v1` is dropped.
- The base URL must use HTTPS unless it names `localhost`, a `.local` host, or a loopback, RFC 1918, link-local or IPv6 unique-local address. It must not embed credentials, because the key is sent as a bearer token.
- The provider calls `GET /key/info`. Then it calls `GET /user/info?user_id=…` for a key bound to a user, or `GET /team/info?team_id=…` for a team-only key. A response whose user or team id differs from the key's is rejected.
- The personal budget is the primary lane and the key's team budget is the secondary lane. With only one budget, it takes the primary lane under its own label. A key with no budget shows "No budget set".
- Amounts such as `$25.00 / $100.00` are detail lines, apart from a real reset date.
- The Automatic tray and float bar metric shows the team budget unless a budget is exhausted.
- Spend without a budget stays visible as API spend. Budget resets give no pace.

## Not ported

Upstream's WidgetKit provider-widget settings have no Windows version. Win-CodexBar has no WidgetKit extension; provider cards and tray entries already work on their own.

## Upstream provider docs

Upstream `docs/providers.md` is a large table of fallback orders for the macOS app. Use it for ideas when you port a provider. For what Windows actually does, read:

1. `rust/src/core/provider/id.rs` (`ProviderId`) and `rust/src/core/provider/spec.rs` (its spec row)
2. `rust/src/providers/<id>/`
3. the output of `codexbar usage -p <id> -v`, or the error on the provider's Settings page

## Related docs

- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [CLI.md](./CLI.md)
- [CONFIGURATION.md](./CONFIGURATION.md)
- [COOKIES.md](./COOKIES.md)
