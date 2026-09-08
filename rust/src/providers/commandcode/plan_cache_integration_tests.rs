use super::*;
use serde_json::json;

fn fixed_time(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("valid fixed timestamp")
}

#[test]
fn recognized_dated_plan_is_reused_as_synthetic_payload() {
    let cache = Mutex::new(CommandCodePlanCache::default());
    let now = fixed_time(1_780_000_000);
    let period_end = fixed_time(1_780_100_000);
    let payload = json!({
        "data": {
            "planId": "individual-goat",
            "currentPeriodEnd": period_end.to_rfc3339(),
        }
    });

    assert_eq!(
        resolve_subscription_payload_with_cache(Ok(payload.clone()), "fingerprint-a", now, &cache,),
        Some(payload)
    );

    let recovered = resolve_subscription_payload_with_cache(
        Err(ProviderError::Timeout),
        "fingerprint-a",
        now,
        &cache,
    )
    .expect("cached subscription");
    assert_eq!(
        recovered,
        json!({
            "data": {
                "planId": "individual-goat",
                "currentPeriodEnd": period_end.to_rfc3339(),
            }
        })
    );
    assert!(recovered.get("status").is_none());
}

#[test]
fn different_fingerprint_cannot_reuse_plan() {
    let cache = Mutex::new(CommandCodePlanCache::default());
    let now = fixed_time(1_780_000_000);
    let payload = json!({
        "data": {
            "planId": "individual-goat",
            "currentPeriodEnd": fixed_time(1_780_100_000).to_rfc3339(),
        }
    });

    resolve_subscription_payload_with_cache(Ok(payload), "fingerprint-a", now, &cache);
    assert_eq!(
        resolve_subscription_payload_with_cache(
            Err(ProviderError::Timeout),
            "fingerprint-b",
            now,
            &cache,
        ),
        None
    );
}

#[test]
fn null_data_clears_prior_plan() {
    let cache = Mutex::new(CommandCodePlanCache::default());
    let now = fixed_time(1_780_000_000);
    let fingerprint = "fingerprint-a";
    let payload = json!({
        "data": {
            "planId": "individual-goat",
            "currentPeriodEnd": fixed_time(1_780_100_000).to_rfc3339(),
        }
    });

    resolve_subscription_payload_with_cache(Ok(payload), fingerprint, now, &cache);
    assert_eq!(
        resolve_subscription_payload_with_cache(
            Ok(json!({"data": null})),
            fingerprint,
            now,
            &cache,
        ),
        Some(json!({"data": null}))
    );
    assert_eq!(
        resolve_subscription_payload_with_cache(
            Err(ProviderError::Timeout),
            fingerprint,
            now,
            &cache,
        ),
        None
    );
}

#[test]
fn unknown_plan_clears_prior_plan_and_returns_original_payload() {
    let cache = Mutex::new(CommandCodePlanCache::default());
    let now = fixed_time(1_780_000_000);
    let fingerprint = "fingerprint-a";
    let recognized = json!({
        "data": {
            "planId": "individual-goat",
            "currentPeriodEnd": fixed_time(1_780_100_000).to_rfc3339(),
        }
    });
    let unknown = json!({
        "data": {
            "planId": "individual-future",
            "currentPeriodEnd": fixed_time(1_780_200_000).to_rfc3339(),
        },
        "status": "active",
    });

    resolve_subscription_payload_with_cache(Ok(recognized), fingerprint, now, &cache);
    assert_eq!(
        resolve_subscription_payload_with_cache(Ok(unknown.clone()), fingerprint, now, &cache,),
        Some(unknown)
    );
    assert_eq!(
        resolve_subscription_payload_with_cache(
            Err(ProviderError::Timeout),
            fingerprint,
            now,
            &cache,
        ),
        None
    );
}

#[test]
fn recognized_plan_without_period_end_is_not_reusable() {
    let cache = Mutex::new(CommandCodePlanCache::default());
    let now = fixed_time(1_780_000_000);
    let fingerprint = "fingerprint-a";
    let payload = json!({"data": {"planId": "individual-goat"}});

    assert_eq!(
        resolve_subscription_payload_with_cache(Ok(payload.clone()), fingerprint, now, &cache,),
        Some(payload)
    );
    assert_eq!(
        resolve_subscription_payload_with_cache(
            Err(ProviderError::Timeout),
            fingerprint,
            now,
            &cache,
        ),
        None
    );
}
