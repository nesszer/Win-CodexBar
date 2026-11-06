# Codex Prometheus 监控示例

用同一 Windows 用户运行 Codex CLI 和发布包中的 `codexbar-cli.exe`，确保能读取该用户的 OAuth 凭据。将 `YOUR_RANDOM_TOKEN` 替换为长随机令牌、`YOUR_LAN_IP` 替换为节点实际局域网 IP 后启动：

```cmd
set CODEXBAR_DASHBOARD_TOKEN=YOUR_RANDOM_TOKEN
codexbar-cli.exe serve --host YOUR_LAN_IP --port 8081 --metrics --allow-plain-http
```

仅允许 Prometheus 主机访问该端口，并在可信局域网中传输令牌。`/metrics` 默认关闭，启用后与 `/usage` 共用 Bearer 认证。`--host` 应设为节点自身的局域网 IP，Prometheus target 应指向该 IP。验证：

```cmd
curl.exe -H "Authorization: Bearer %CODEXBAR_DASHBOARD_TOKEN%" http://YOUR_LAN_IP:8081/metrics
```

将 [抓取示例](codexbar-scrape-example.yml)中的示例地址替换成实际节点 IP，并把 `scrape_configs` 和 [告警规则](codexbar-alerts.yml)的路径加入现有 `prometheus.yml`；示例规则默认匹配 `job="codexbar"`。多台节点共用一个 job 时必须使用同一个 Bearer Token；若令牌不同，则为每台节点建一个单独 job，并同步调整告警规则中的 job 选择器。运行 `promtool check config` 和 `promtool check rules` 后重载 Prometheus。导入 [Grafana 中文面板](../grafana/codexbar-codex-zh.json)，选择 Prometheus 数据源；额度窗口默认 weekly，节点可选单台或全部。

面板采用多节点中文大屏布局：顶部显示抓取、生成、时效和健康状态；中部显示剩余额度、重置卡数量与有效期、Codex 采集状态，以及“距离额度重置趋势”；底部的成本和采集详情默认折叠。倒计时按每个历史采样时刻计算 `max(重置时间 - 采样时间, 0) / 86400`，可观察剩余天数逐步下降。节点筛选支持单台或全部，额度窗口默认 weekly。上游额度指标按窗口命名（例如 `codexbar_quota_weekly_remaining_ratio`），面板自动将 0 到 1 的比例换算为百分比。

`codexbar_reset_credits_available{provider="codex"}` 中，`0` 为已用完，`-1` 为 PAT 模式或接口不可用。`codexbar_reset_credits_next_expiry_timestamp_seconds` 只在有可用卡且存在未来有效期时输出，不暴露卡 ID。重置卡接口查询成功或失败均缓存 10 分钟；异常的周额度重置确认会获取独立观察。配额指标以 0 到 1 的比例表示，Grafana 面板会按百分比显示。成本来自本地日志估算，不是订阅账单。

## 订阅结束时间

“Codex 订阅信息”表仅显示“节点、订阅结束时间”两列，独立于额度窗口筛选。结束时间按“接口到期时间 → 接口续费时间 → 登录记录中的周期结束时间”取值；自动续费账号显示下一次续费日期，不表示届时停用。登录记录可能滞后，具体含义在表格说明中提示。开始时间不再由面板查询或显示，指标仍分别保留以下字段，供需要详细信息的查询使用：

| 订阅字段 | Prometheus 指标 | 含义 |
| --- | --- | --- |
| `startsAt` | `codexbar_subscription_start_timestamp_seconds{provider="codex"}` | 接口明确提供的订阅开始时间 |
| `expiresAt` | `codexbar_subscription_expiry_timestamp_seconds{provider="codex"}` | 接口明确提供的订阅到期时间；已过期的日期仍保留 |
| `renewsAt` | `codexbar_subscription_renewal_timestamp_seconds{provider="codex"}` | 自动续费账号下一次续费时间；独立于到期时间 |
| 登录记录中的周期终点 | `codexbar_subscription_period_end_timestamp_seconds{provider="codex"}` | `id_token` 明确提供的 `chatgpt_subscription_active_until`；不等于确认的取消续费到期时间 |
| 日期来源 | `codexbar_subscription_dates_from_id_token{provider="codex"}` | `1` 为登录记录，`0` 为订阅接口；无日期时省略 |
| 日期记录时间 | `codexbar_subscription_dates_recorded_timestamp_seconds{provider="codex"}` | 登录声明的签发时间 `iat`；不是监控采集时间 |

指标值是 Unix 秒时间戳，Grafana 查询乘以 1000 后显示日期，时间沿用仪表盘时区（默认浏览器本地时区）。这些字段来自现有采集结果，抓取 `/metrics` 不会另外请求订阅接口。

需开启设置中的“OpenAI 网页附加功能”（默认开启）。订阅查询使用现有 OAuth 凭据，在 `/backend-api/subscriptions` 同时传入 URL 编码后的 `account_id` 查询参数与相同的 `ChatGPT-Account-Id` 请求头；CLI 和托管账号路径一致。仅传请求头可能返回 HTTP 400。查询保留 8 秒超时及失败降级，不新增抓取请求或凭据刷新。

订阅接口成功时始终优先，包括明确返回空订阅的情况。自动续费开启时，`active_until` 表示下一次续费时间 `renewsAt`，不会冒充 `expiresAt`；面板会使用独立的续费指标，避免继续展示旧登录周期。

当订阅接口不可用时，OAuth 用量采集成功后可使用同一 `auth.json` 的 `id_token` 中明确记录的周期：`chatgpt_subscription_active_start` 作为开始时间，`chatgpt_subscription_active_until` 作为单独的“周期结束时间”。只有令牌声明的账号 ID 与当前凭据账号 ID 一致才接受；身份缺失、不匹配、日期格式错误或周期倒置时均忽略。PAT、API Key 和自定义后端不使用此降级来源。

登录记录只反映令牌签发时的订阅周期，可能滞后，不能据此确认自动续费、取消订阅或当前订阅状态。来源和记录时间仍可通过上述指标查询；面板只显示节点和结束时间。旧周期结束日期仍保留，不能仅凭它告警“账号已到期”。不读取或更改刷新令牌、不触发登录，不增加网络请求。新的周期信息在 Codex 自己更新凭据后随下一次正常采集读取。

登录周期仅通过 `/metrics` 导出，不写入持久化或通用 `subscription.expiresAt` 字段，也不进入桌面订阅到期提示。未取得日期时省略对应样本，表格显示“未提供”；这不代表账号已过期。不要根据额度重置、重置卡有效期、JWT `exp` 或续费时间推算订阅到期时间。

接口说明见 [OpenAPI 文档](openapi.yaml)。

## 面板查询校验

节点和采集任务筛选使用 Prometheus 数据源默认插值 `${instance}`、`${job}`，由数据源同时处理正则和 PromQL 字符串的转义。不要在引号内改为通用 `${instance:regex}`、`${job:regex}`：Grafana 10.4.3 会为 IP 中的点生成单个反斜杠，导致 `unknown escape sequence`，单节点和“全部”筛选都可能失败。

在仓库根目录运行 `python docs/grafana/test_queries.py`，验证单节点、多节点、全部节点和含点任务名的展开结果。此用例会先复现通用 `:regex` 格式的错误，再验证面板配置；发布前还应在实际 Grafana 中验证筛选和数据展示。
