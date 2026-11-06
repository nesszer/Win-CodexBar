"""复现 Grafana 多选变量的转义行为，校验最终 PromQL 字符串而非预先补转义。"""

import json
from pathlib import Path
import re
import unittest


def expand_variable(values, formatter):
    # 通用 :regex 只处理正则层；Prometheus 默认格式还处理 PromQL 字符串层。
    patterns = [re.sub(r"[\\$^*{}\[\]+?.()|]", lambda match: "\\" + match[0], value)
                for value in values]
    pattern = patterns[0] if len(patterns) == 1 else "(" + "|".join(patterns) + ")"
    if formatter == "regex":
        return pattern
    return pattern.replace("\\", "\\\\").replace('"', '\\"')


def expand_query(query, variables, window):
    query = query.replace("${window:raw}", window)
    return re.sub(
        r"\$\{(job|instance)(?::(regex))?\}",
        lambda match: expand_variable(variables[match[1]], match[2]),
        query,
    )


def queries(document):
    if isinstance(document, dict):
        for key, value in document.items():
            if key in ("expr", "definition", "query") and isinstance(value, str) and "${" in value:
                yield value
            else:
                yield from queries(value)
    elif isinstance(document, list):
        for value in document:
            yield from queries(value)


class DashboardQueriesTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.dashboard = json.loads(Path(__file__).with_name("codexbar-codex-zh.json").read_text(encoding="utf-8"))

    def test_query_literals_for_single_multiple_and_all_nodes(self):
        nodes = ["192.168.13.111:8081", "192.168.13.184:8081", "192.168.13.241:8081"]
        for window in ("weekly", "session", "monthly", "code_review"):
            for jobs in (["codex-bar"], ["codex.bar", "codex-other"]):
                for selected in (nodes[:1], nodes[:2], nodes):
                    for query in queries(self.dashboard):
                        with self.subTest(window=window, jobs=jobs, nodes=selected, query=query):
                            expanded = expand_query(query, {"job": jobs, "instance": selected}, window)
                            self.assertNotIn("${", expanded)
                            # 此处 IP 与正则的转义在 JSON 和 PromQL 双引号字符串中具有相同语法。
                            for operator, literal in re.findall(r'(=~|!~|!=|=)\s*("(?:\\.|[^"\\])*")', expanded):
                                pattern = json.loads(literal)
                                if operator in ("=~", "!~"):
                                    re.compile(pattern)

    def test_default_interpolation_matches_literal_ips(self):
        expanded = expand_query(
            self.dashboard["panels"][0]["targets"][0]["expr"],
            {"job": ["codex-bar"], "instance": ["192.168.13.111:8081", "192.168.13.241:8081"]},
            "weekly",
        )
        literal = re.search(r'instance=~("(?:\\.|[^"\\])*")', expanded)[1]
        pattern = json.loads(literal)
        self.assertIsNotNone(re.fullmatch(pattern, "192.168.13.111:8081"))
        self.assertIsNotNone(re.fullmatch(pattern, "192.168.13.241:8081"))
        self.assertIsNone(re.fullmatch(pattern, "192a168b13c111:8081"))

    def test_explicit_regex_formatter_reproduces_reported_failure(self):
        expanded = expand_query(
            'up{instance=~"${instance:regex}"}',
            {"instance": ["192.168.13.111:8081"]},
            "weekly",
        )
        with self.assertRaises(json.JSONDecodeError):
            json.loads(expanded[len('up{instance=~'):-1])

    def test_subscription_table_only_shows_node_and_end(self):
        # 单查询直接整理节点和结束时间，不再连接开始时间或保留其列映射。
        panel = next(panel for panel in self.dashboard["panels"] if panel.get("title") == "Codex 订阅信息")
        self.assertEqual([target["refId"] for target in panel["targets"]], ["A"])
        end_query = panel["targets"][0]["expr"]
        self.assertLess(end_query.index("codexbar_subscription_expiry_timestamp_seconds"),
                        end_query.index("codexbar_subscription_renewal_timestamp_seconds"))
        self.assertLess(end_query.index("codexbar_subscription_renewal_timestamp_seconds"),
                        end_query.index("codexbar_subscription_period_end_timestamp_seconds"))
        self.assertIn("or on(instance)", end_query)
        self.assertNotIn("dates_from_id_token", end_query)
        self.assertEqual([step["id"] for step in panel["transformations"]], ["organize"])
        names = panel["transformations"][0]["options"]["renameByName"]
        self.assertEqual(names, {"instance": "节点", "Value": "订阅结束时间"})
        self.assertNotIn("订阅开始时间", json.dumps(panel, ensure_ascii=False))
        self.assertNotIn("codexbar_subscription_start_timestamp_seconds", end_query)
        self.assertIn("登录记录可能滞后", panel["description"])


if __name__ == "__main__":
    unittest.main()
