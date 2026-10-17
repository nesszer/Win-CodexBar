# Codex Prometheus 监控示例

用同一 Windows 用户运行 Codex CLI 和 `codexbar`，确保能读取该用户的 OAuth 凭据。将 `YOUR_RANDOM_TOKEN` 替换为长随机令牌、`YOUR_LAN_IP` 替换为节点实际局域网 IP 后启动：

```cmd
set CODEXBAR_DASHBOARD_TOKEN=YOUR_RANDOM_TOKEN
codexbar.exe serve --host YOUR_LAN_IP --port 8081 --metrics --allow-plain-http
```

仅允许 Prometheus 主机访问该端口，并在可信局域网中传输令牌。`/metrics` 默认关闭，启用后与 `/usage` 共用 Bearer 认证。`--host` 应设为节点自身的局域网 IP，Prometheus target 应指向该 IP。验证：

```cmd
curl.exe -H "Authorization: Bearer %CODEXBAR_DASHBOARD_TOKEN%" http://YOUR_LAN_IP:8081/metrics
```

将 [抓取示例](codexbar-scrape-example.yml)中的示例地址替换成实际节点 IP，并把 `scrape_configs` 和 [告警规则](codexbar-alerts.yml)的路径加入现有 `prometheus.yml`；示例规则默认匹配 `job="codexbar"`。多台节点共用一个 job 时必须使用同一个 Bearer Token；若令牌不同，则为每台节点建一个单独 job，并同步调整告警规则中的 job 选择器。运行 `promtool check config` 和 `promtool check rules` 后重载 Prometheus。导入 [Grafana 中文面板](../grafana/codexbar-codex-zh.json)，选择 Prometheus 数据源；额度窗口默认 weekly，节点可选单台或全部。

面板采用多节点中文大屏布局：顶部显示抓取、生成、时效和健康状态；中部显示剩余额度、重置卡数量与有效期、Codex 采集状态，以及“距离额度重置趋势”；底部的成本和采集详情默认折叠。倒计时按每个历史采样时刻计算 `max(重置时间 - 采样时间, 0) / 86400`，可观察剩余天数逐步下降。节点筛选支持单台或全部，额度窗口默认 weekly。上游额度指标按窗口命名（例如 `codexbar_quota_weekly_remaining_ratio`），面板自动将 0 到 1 的比例换算为百分比。

`codexbar_reset_credits_available{provider="codex"}` 中，`0` 为已用完，`-1` 为 PAT 模式或接口不可用。`codexbar_reset_credits_next_expiry_timestamp_seconds` 只在有可用卡且存在未来有效期时输出，不暴露卡 ID。重置卡接口查询成功或失败均缓存 10 分钟；异常的周额度重置确认会获取独立观察。配额指标以 0 到 1 的比例表示，Grafana 面板会按百分比显示。成本来自本地日志估算，不是订阅账单。
