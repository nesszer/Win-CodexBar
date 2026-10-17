# Exa Team Management API Provider (PR #842)

## Summary

Port of Exa Team Management API usage provider from upstream CodexBar v0.74.1. Displays selected API key monthly spend (UTC) for team members monitoring their individual usage costs.

## Changes

- Added Exa provider module (`rust/src/providers/exa/mod.rs`)
- Wired Exa to core provider factory and settings
- Added frontend icons and provider catalog entries
- Added comprehensive test suite (11 tests: 8 behavior, 3 metadata)
- Full Windows parity with upstream v0.74.1 implementation

## Upstream References

- **Plugin:** `Resources/Plugins/exa.js` (v0.74.1, commit fca039015)
- **Docs:** `docs/exa.md` (service key auth, team management API, monthly spend endpoint)
- **Tests:** Upstream test cases for valid/zero/invalid spend, period validation, error handling

## Implementation Details

### Authentication
- **Service Key**: Team Management service key (x-api-key header)
- **API Key ID**: Workspace identifier for the specific key to monitor
- **Settings:** Stored via provider_configs.exa.workspace_id + api_keys catalog

### Endpoint
- **URL:** `GET /team-management/api-keys/{id}/usage` at `https://admin-api.exa.ai`
- **Query params:** start_date (UTC month start), end_date (query time, rounded down to seconds)
- **Response:** total_cost_usd (finite, non-negative), period.start/end (must match request exactly)

### Display
- **Tray panel:** Exa card showing "Service key" login method + spend detail
- **Detail rows:** "API key this month (UTC)" section title, "Spend" label, formatted USD value
- **Source label:** "api" (API-key authentication model)
- **No secondary quota:** Exa only exposes spend, not request limits or quotas

### Error Handling
- **401 Unauthorized:** "Exa rejected the service key. Ordinary search API keys are not supported."
- **403 Forbidden:** "Exa Team Management must be enabled and both keys must belong to the same team."
- **429 Rate Limited:** Standard rate limit error
- **5xx Server Error:** Provider unavailability message
- **Other errors:** Generic API failure message

### Validation
- API key ID must not be empty or "." or ".."
- Period dates in response must match request exactly (strict validation per upstream)
- Cost must be a finite, non-negative number
- Unknown stays unknown (no inferred balances, resets, or allowances)

## Test Coverage

### Unit Tests (8 tests)
- `test_parse_valid_usage`: Success case with $45.67 spend
- `test_parse_zero_spend`: Zero spend visibility
- `test_parse_rejects_missing_cost`: Missing total_cost_usd field
- `test_parse_rejects_negative_cost`: Negative spend rejection
- `test_parse_rejects_nonfinite_cost`: Non-finite (null, string, array, etc.) rejection
- `test_parse_rejects_mismatched_period`: Period date validation

### Metadata Tests (3 tests)
- Provider is registered in ProviderId::all()
- CLI name: "exa"
- Display name: "Exa"
- from_cli_name("exa") returns Some(Exa)
- No cookie domain (API-key provider)

### Real-Window Proof (3 scenarios)
- **Success case:** $45.67 spend with correct labels, login method, detail section
- **401 error:** Friendly unauthorized message, no body leak
- **403 error:** Team mismatch guidance, no body leak

## Checks Passed

```
fmt-check exit=0
rust-test exit=0 (4161 tests passed, 11 Exa-specific)
rust-clippy exit=0
tauri-test exit=0
tauri-clippy exit=0
pnpm-test exit=0
tsc exit=0
build exit=0
```

## Proof Files

- **proof/1-tray-panel-success.png** — Exa card with $45.67 spend
- **proof/2-error-401.png** — 401 unauthorized with friendly message
- **proof/3-error-403.png** — 403 team mismatch error
- **proof/mock-success.log** — Mock server 200 response
- **proof/mock-401.log** — Mock server 401 response (with shell retry)
- **proof/mock-403.log** — Mock server 403 response (with shell retry)
- **proof/pack/Exa/**, **Exa-Settings/**, **Exa-Error-401/**, **Exa-Error-403/** — Settings packs

## Windows Differences from Mac

None identified. Implementation matches upstream v0.74.1 exactly:
- Rust mechanics (HTTP client, date formatting, error handling)
- Strict date validation (must match request exactly)
- Only spend exposed (no quota, remaining balance, or inferred team totals)
- Service key + API key ID model (no cookies, no OAuth)
- Source label "api" per Windows provider pattern

## Stacked On

PR #840 (feat/provider-tavily, 285d59f0b). Merge in lane order: #840 then #842.

## Status

- [x] All files wired (core, settings, icons, catalog, strings, README, docs)
- [x] Tests passing (11 tests, all green)
- [x] Full local checks passing (fmt, clippy, cargo test, pnpm, tsc, build)
- [x] Real-window proof with mock server (3 test cases, all pass)
- [x] No real endpoints contacted
- [x] Error handling verified
- [x] Ready for review
