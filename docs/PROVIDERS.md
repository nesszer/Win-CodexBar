# Providers

This page explains how a provider is added and fetched in Win-CodexBar, how its usage windows and pace work, and what is special about individual providers. The upstream `docs/providers.md` describes the macOS app. Its ids and fallback order differ from this port, so check the code before you copy anything from it.

## Add a provider

1. Add a `ProviderId` variant in `rust/src/core/provider.rs`, with its metadata: `cli_name`, `display_name`, `from_cli_name` and, if it uses cookies, the cookie domain.
2. Implement the `Provider` trait in `rust/src/providers/<name>/`.
3. Add a match arm in `instantiate` in `rust/src/core/provider_factory.rs`.
4. Add a match arm in `pace_capability` in the same file. Map the provider to its own `PACE_CAPABILITY`, or to `ProviderPaceCapability::UNSUPPORTED`. See [Pace](#pace).

Both matches are exhaustive, so a missing arm doesn't compile. A test checks that every id can be built.

Rules:

- `instantiate_provider` is the only provider factory. The desktop shell and the CLI both use it. Don't write another one.
- Keep parsing, auth and display choices inside the provider module. Shared code has no `if provider == X` branches. When shared code needs a provider-specific answer, add a typed field or a trait method with a default, as `menu_bar_pace_lane` and `reset_regenerates` below do.
- Keep identity, plan and email per provider. Never show one provider's account data on another provider's card or page.

`ProviderId` has 96 variants. A fresh install enables only Claude and Codex. Kimi K2 and CrossModel are deprecated and stay hidden unless they are already enabled.

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
| Grok | a window labelled weekly whose reset falls inside it | no |
| Z.ai | the 43200-minute MCP window | yes |
| Amp | a window with a length whose reset text starts with "renews in " | no |
| Alibaba, Alibaba Token Plan, Command Code, Doubao, MiMo, StepFun, Notion, Ollama, OpenCode Go | exactly 43200 minutes | yes |

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
- Regenerating lanes keep their length (300 and 10080 minutes) and set
  `reset_regenerates`. The card counts them down as "Regenerates in 45m"
  instead of "Resets in", always as a relative time, and shows no pace line
  for them. Plain lanes keep "Resets in". The tray pace tint still scores a
  regenerating lane.
- The plan name (`plan`, `planName`, `tier`, ...) is shown as the plan.
- 401 and 403 report "Invalid Synthetic API credentials."; response bodies are
  never echoed in errors.

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

## Window lengths that match the Mac

These providers set window lengths the same way the Mac app does, so their rows get pace and the tray tint can score them:

| Provider | Window | Length |
| --- | --- | --- |
| Codebuff | Weekly lane | 10080 minutes. The Credits lane has no length. |
| MiniMax | Services rows from the coding-plan page | From the row's window type, for example 300 for 5 hours and 10080 for Weekly |
| MiniMax | Remains weekly rows | Always 10080 minutes, even without a start and end |
| Synthetic | Regenerating five-hour and weekly lanes | 300 and 10080 minutes, from the payload |
| Grok | Credits proxy period | Kept after the period ends. An ended period has no pace. |
| Z.ai | `TIME_LIMIT` windows | From the unit: 1 = 1440, 3 = 60, 5 = 1, 6 = 10080 minutes per unit. The monthly MCP marker keeps the 30-day sentinel. |
| Notion | Billing lane | Labelled "Monthly". Other monthly lanes keep the "30-Day" label. |

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
| Statuspage (`/api/v2/status.json`) | Claude, Codex and OpenAI API (status.openai.com), Copilot (githubstatus.com), Cursor, Factory, Augment, Hugging Face, ZoomMate (zoomstatus.com) |
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

OpenCode Go's local SQLite reader reads only its own `opencode-go` assistant records. OpenAI API-platform usage is a separate provider.

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

## API key notes

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

1. `rust/src/core/provider.rs` (`ProviderId`)
2. `rust/src/providers/<id>/`
3. the output of `codexbar usage -p <id> -v`, or the error on the provider's Settings page

## Related docs

- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [CLI.md](./CLI.md)
- [CONFIGURATION.md](./CONFIGURATION.md)
- [COOKIES.md](./COOKIES.md)
