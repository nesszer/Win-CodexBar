# CodexBar Windows 配置管理工具

排查 CodexBar(Windows / Tauri 版)provider 问题时产出的一组本地运维脚本,核心场景是
**解密 / 修复 `%APPDATA%\CodexBar\` 下的 DPAPI 配置文件**(2026-09-16,commandcode
provider 额度显示问题)。

## 背景:配置文件格式

敏感配置(`settings.json`、`api_keys.json`、`manual_cookies.json`、`token-accounts.json`)
都是 `codexbar.secure-file` 格式(`protection: windows-dpapi-user`):

```text
{ "format": "codexbar.secure-file", "version": 1, "protection": "windows-dpapi-user",
  "payload": "<base64>" }
```

`payload` base64 解码后用 DPAPI `CurrentUser` 解密,得到明文 JSON。

**编辑三原则**(违反会直接把配置弄坏,或被运行中的应用覆盖):

1. 先 `Stop-Process codexbar* -Force` 再改文件,改完再启动(应用持有内存态,退出/刷新时会回写);
2. 写回必须 **UTF-8 无 BOM**(PowerShell 5.1 的 `Set-Content -Encoding UTF8` 带 BOM,Rust serde 解析失败后会静默回退默认值);
3. 动手前留 `.bak`。

## commandcode(Command Code)provider 可用配置

两个文件必须**同时**存在对应条目:

1. `token-accounts.json` → `providers.commandcode.accounts[].token` = 裸 token 值
   (DevTools → Application → Cookies → `__Secure-commandcode_prod_.session_token` 的值,URL 编码原样,`%2F`/`%3D` 不要解码);
2. `manual_cookies.json` → `cookies.commandcode.cookie_header` = **完整表单**
   `__Secure-commandcode_prod_.session_token=<token>`。

API:`GET https://api.commandcode.ai/internal/billing/credits`(及 `/internal/billing/subscriptions`),
需带 `Origin: https://commandcode.ai` / `Referer: https://commandcode.ai/`。

注意:该站点 better-auth 实例名为 `commandcode_prod_`,用 `better-auth.session_token`
这个名字发同一个 token 会 **401 "You're logged out"**。

## 已确认的坑(v0.60.x,2026-09-16)

1. **`codexbar-cli.exe usage -p commandcode` 不读 manual_cookies**,只扫浏览器 cookie 库;
   更糟的是它会把 `manual_cookies.json` 里 commandcode 的 `cookie_header` **剥掉 cookie 名**写回
   (完整表单 → 裸值),之后所有拉取 401。测试 provider 状态只能用
   `codexbar-cli.exe diagnose -p commandcode`。
2. 桌面应用请求时,裸 token 会被套上错误的 cookie 名(推测 `better-auth.session_token`)
   → 401 → 托盘显示"需要登录"。(修复脚本的本质:保证完整表单的 manual cookie 生效)
3. 应用拉取失败(401)后会把 `manual_cookies.json` 里对应 provider 的条目**删掉**,
   排查时先检查条目还在不在。

诊断技巧:桌面应用支持 `RUST_LOG=debug` 环境变量,日志(`logs\codexbar-desktop.log`)
会输出 reqwest 连接 / provider 刷新明细;provider 失败会有
`preserving last good provider snapshot ... error=...` 行,只有 `slow provider refresh` 而无 error 行 = 拉取成功。

## 脚本清单

| 脚本 | 用途 |
|---|---|
| `read-all-configs.ps1` | 解密并列出 settings / api_keys / manual_cookies |
| `read-token-accounts.ps1` | 解密 token-accounts.json |
| `read-provider-configs.ps1` | 只看 settings.json 的 provider_configs / enabled_providers |
| `fix-commandcode-cookie.ps1` | 停应用 → 从 token-accounts 读 token → 重建完整表单 manual cookie → 校验 |
| `debug-launch.ps1` | 恢复 commandcode 已知可用状态并以 `RUST_LOG=debug` 启动应用 |
| `research/strings-dump.ps1` | 从 codexbar.exe 提取字符串上下文(定位 provider 端点 / cookie 名候选) |
| `research/decrypt-chromium-cookies.py` | 解密 Chromium 系浏览器 Cookies(v10 可解;Edge v20 app-bound 加密会被拒) |
| `research/ocr-screenshot.ps1` | Windows 本地 WinRT OCR(截图中提取文字,不出机器) |
| `research/find-webview2-profiles.ps1` | 列出各应用 WebView2 user-data-dir 与进程 |

## 使用

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\win-config-tools\read-all-configs.ps1
```

所有脚本只操作当前用户(`%APPDATA%` / DPAPI CurrentUser),不需要管理员权限。
