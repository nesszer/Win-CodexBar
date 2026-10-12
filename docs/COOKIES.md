# Browser cookies on Windows

Some providers sign in through their website, for example Claude, Cursor and Kimi. For these providers CodexBar can read the session cookie from a browser on the same PC, or you can paste the cookie yourself. On Windows, CodexBar uses DPAPI and browser profile folders. It doesn't use the macOS Keychain.

## Import cookies for a provider

1. Open **Settings > Providers** and select the provider.
2. Find the **Browser Cookies** section in the provider pane. It appears only for providers that sign in with web cookies.
3. Pick a browser and profile set from the list. The list shows only browsers that CodexBar found on this PC, with their profile count.
4. Click **Import from Browser**.

CodexBar reads the cookies for that provider's domain and saves them as the provider's manual cookie. The section then shows a **Saved** badge and a **Remove** button.

The provider's **Cookie source** setting decides how CodexBar uses cookies during a refresh:

- **Automatic** reads the browser cookies on each refresh.
- **Manual** uses only the cookie you pasted or imported.
- **Off** turns cookie sign-in off for that provider.

Some providers add their own steps to Automatic. The help text under the setting says what each one reads.

## Paste a cookie by hand

Use this when automatic import fails. For example, the browser uses App-Bound Encryption, the cookie database can't be read, or CodexBar runs in WSL.

1. Open the provider's website in your browser and sign in, for example `claude.ai`.
2. Open DevTools (F12) and select the **Network** tab.
3. Reload the page and select any request to the provider.
4. Under **Request Headers**, copy the value of the `Cookie` header.
5. In **Settings > Providers > (provider) > Browser Cookies**, paste the value and click **Save Cookie**.

Manual cookies are stored in `manual_cookies.json` in the CodexBar config folder. The file is written through `secure_file`, which uses DPAPI on Windows. See [CONFIGURATION.md](./CONFIGURATION.md).

## Supported browsers

| Browser | Cookie storage | Automatic import |
| --- | --- | --- |
| Chrome | DPAPI and AES-256-GCM. Newer profiles may use App-Bound Encryption (`v20`). | Works only when the needed cookies are not App-Bound. |
| Chrome Beta, Dev, Canary and Chrome for Testing | Same as Chrome | Same as Chrome |
| Chromium | Same as Chrome | Same as Chrome |
| Edge | Same as Chrome | Same as Chrome |
| Brave | Same as Chrome | Same as Chrome |
| Arc | Same as Chrome | Same as Chrome |
| Firefox | Unencrypted SQLite | Works |

Each Chrome channel, Chromium, Edge, Brave and Arc is a separate choice. Each one reads only its own `User Data` profiles. Firefox reads the profiles under `%APPDATA%\Mozilla\Firefox\Profiles`.

## App-Bound Encryption

Chromium App-Bound Encryption (ABE) ties the cookie key to the browser install. CodexBar doesn't bypass it. If the selected profile stores the provider's cookies as App-Bound `v20` values, CodexBar can't decrypt them with the user's DPAPI key.

Closing the browser doesn't remove ABE. Paste the cookie by hand, or sign in to the provider in Firefox and import from there. The Chromium choices stay in the list because older profiles can still hold readable DPAPI and AES-GCM cookies.

## Kimi tokens from local storage

When the Kimi cookie source is Automatic, CodexBar also reads `access_token` from the `Local Storage` of Chromium browsers, for the selected Kimi region (`www.kimi.com` or `www.kimi.ai`). It tries this after the Kimi Desktop session and the browser cookies.

- Local storage is not App-Bound encrypted, so this can work when cookie decryption is blocked.
- CodexBar uses only unexpired JWTs with three segments. It never reads refresh tokens.
- A manual cookie always wins. Cookie source **Manual** or **Off** skips this step.
- CodexBar scans only the `Default` and `Profile N` profiles. It doesn't read Firefox local storage.

If the token has expired, open Kimi in the browser to renew the session.

## Troubleshooting

| Message or symptom | What to do |
| --- | --- |
| "... Chromium App-Bound Encryption ... blocking automatic cookie import" | Paste the cookie by hand, or import from Firefox. |
| "Database error" or "Decryption error" | Close the browser and try again, in case the cookie database is locked. |
| "No cookies found for (domain)" | Sign in to the provider's website in that browser, then import again. |
| The browser list is missing | CodexBar found no supported browser. Paste the cookie by hand. |
| CodexBar runs in WSL | WSL can't decrypt Chromium DPAPI cookies. Paste the cookie by hand or use CLI sign-in. See [WSL.md](./WSL.md). |

## Related

- [CONFIGURATION.md](./CONFIGURATION.md): where manual cookies and settings are stored.
- [PROVIDERS.md](./PROVIDERS.md): which providers use web, CLI or OAuth sign-in.
- [WSL.md](./WSL.md): why Chromium cookie decryption fails in WSL.
