# Providers (Windows)

Windows rewrite of the *role* of upstream `docs/providers.md`: how providers are registered and fetched in **this** repo.
Do **not** treat upstream’s full strategy table as authoritative for Win-CodexBar without checking code — IDs and auto-order drift.

## Single factory

All shells and the CLI construct providers through:

```text
codexbar::core::instantiate_provider  →  rust/src/core/provider_factory.rs
```

`ProviderId` lives in `rust/src/core/provider/id.rs`; its names, aliases, brand color and metadata are one row in `rust/src/core/provider/spec.rs`. The factory match is **exhaustive** (missing arm = compile error). Tests ensure every id instantiates.

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

1. `rust/src/core/provider/id.rs` (`ProviderId`) and `rust/src/core/provider/spec.rs` (its spec row)
2. `rust/src/providers/<id>/`
3. `codexbar usage -p <id> -v` / desktop provider detail errors

## Related

- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [CLI.md](./CLI.md)
- [CONFIGURATION.md](./CONFIGURATION.md)
- [COOKIES.md](./COOKIES.md)
