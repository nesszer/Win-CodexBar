use crate::commands::ProviderUsageSnapshot;
use codexbar::core::{
    ProviderFetchResult, ProviderId, ProviderThroughput, RateWindow, ThroughputMinute,
    ThroughputModel, UsageSnapshot, instantiate_provider,
};

fn throughput() -> ProviderThroughput {
    ProviderThroughput {
        minutes: vec![
            ThroughputMinute {
                start: 1_700_179_020,
                total_tokens: 120,
                requests: 3,
                is_pending: false,
            },
            ThroughputMinute {
                start: 1_700_179_080,
                total_tokens: 40,
                requests: 1,
                is_pending: true,
            },
        ],
        frontier: Some(1_700_179_020),
        latest_tokens_per_minute: Some(120),
        peak_tokens_per_minute: 120,
        peak_at: Some(1_700_179_020),
        five_minute_average_tokens_per_minute: 24.0,
        models: vec![ThroughputModel {
            name: "gpt-5".to_string(),
            latest_tpm: 120,
            peak_tpm: 120,
            total_tokens: 160,
            peak_limit_counted_tokens: 100,
            limit_counted_tokens_by_minute: vec![100, 40],
        }],
        window_minutes: 60,
        fetched_at: 1_700_179_105,
        project_id: None,
    }
}

fn snapshot(throughput: Option<ProviderThroughput>) -> ProviderUsageSnapshot {
    let metadata = instantiate_provider(ProviderId::OpenAIApi)
        .metadata()
        .clone();
    let mut result =
        ProviderFetchResult::new(UsageSnapshot::new(RateWindow::new(0.0)), "admin-api");
    result.throughput = throughput;
    ProviderUsageSnapshot::from_fetch_result(ProviderId::OpenAIApi, &metadata, &result, None, None)
}

#[test]
fn fetch_result_throughput_reaches_the_bridge_as_camel_case_epoch_seconds() {
    let json = serde_json::to_value(snapshot(Some(throughput()))).unwrap();
    assert_eq!(
        json["throughput"],
        serde_json::json!({
            "minutes": [
                {"start": 1_700_179_020, "totalTokens": 120, "requests": 3, "isPending": false},
                {"start": 1_700_179_080, "totalTokens": 40, "requests": 1, "isPending": true},
            ],
            "frontier": 1_700_179_020,
            "latestTokensPerMinute": 120,
            "peakTokensPerMinute": 120,
            "peakAt": 1_700_179_020,
            "fiveMinuteAverageTokensPerMinute": 24.0,
            "models": [{
                "name": "gpt-5",
                "latestTpm": 120,
                "peakTpm": 120,
                "totalTokens": 160,
                "peakLimitCountedTokens": 100,
                "limitCountedTokensByMinute": [100, 40],
            }],
            "windowMinutes": 60,
            "fetchedAt": 1_700_179_105,
            "projectId": null,
        })
    );
}

#[test]
fn missing_throughput_is_omitted_and_round_trips() {
    let json = serde_json::to_value(snapshot(None)).unwrap();
    assert!(json.get("throughput").is_none());

    let original = snapshot(Some(throughput()));
    let decoded: ProviderUsageSnapshot =
        serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
    assert_eq!(decoded.throughput, original.throughput);

    let legacy: ProviderUsageSnapshot = serde_json::from_value(json).unwrap();
    assert!(legacy.throughput.is_none());
}
