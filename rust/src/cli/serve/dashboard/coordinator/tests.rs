use super::*;
use crate::cli::serve::collection::SnapshotCollection;
use crate::cli::serve::dashboard::snapshot::{
    DashboardIdentity, ProviderFetchEnvelope, SnapshotInput, build_snapshot,
};
use crate::cli::serve::metrics::MetricsSnapshot;
use crate::core::{ProviderFetchResult, RateWindow, UsageSnapshot};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

fn stub_input() -> SnapshotInput {
    SnapshotInput {
        collection: SnapshotCollection {
            providers: vec![ProviderFetchEnvelope {
                id: "claude".to_string(),
                display_name: "Claude".to_string(),
                session_label: "Session".to_string(),
                weekly_label: "Weekly".to_string(),
                fetch: Ok(ProviderFetchResult::new(
                    UsageSnapshot::new(RateWindow::new(50.0)),
                    "test",
                )),
            }],
            costs: HashMap::new(),
            claude_accounts: None,
            generated_at: chrono::Utc::now(),
            refresh_seconds: 60,
            order: vec![],
            enabled: BTreeSet::new(),
        },
        identity: DashboardIdentity::Redacted,
        version: None,
        usage_bars_show_used: None,
    }
}

fn stub_artifacts() -> SnapshotArtifacts<MetricsSnapshot> {
    let input = stub_input();
    SnapshotArtifacts {
        sidecar: Some(MetricsSnapshot::from_collection(&input.collection)),
        dashboard: build_snapshot(&input),
    }
}

fn counting_source(calls: Arc<AtomicUsize>, delay: Duration) -> SnapshotBuildFn {
    Arc::new(move || {
        let calls = calls.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(delay).await;
            Ok(build_snapshot(&stub_input()))
        })
    })
}

#[tokio::test]
async fn serves_first_build_then_cache_within_ttl() {
    let calls = Arc::new(AtomicUsize::new(0));
    let coordinator = SnapshotCoordinator::new(
        Duration::from_secs(3600),
        counting_source(calls.clone(), Duration::ZERO),
    );
    let first = coordinator.get().await.unwrap();
    let second = coordinator.get().await.unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "second get must use the TTL cache"
    );
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(first.schema_version, 1);
}

#[tokio::test]
async fn expired_ttl_rebuilds() {
    let calls = Arc::new(AtomicUsize::new(0));
    let coordinator = SnapshotCoordinator::new(
        Duration::ZERO,
        counting_source(calls.clone(), Duration::ZERO),
    );
    coordinator.get().await.unwrap();
    coordinator.get().await.unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "zero ttl forces a fresh build"
    );
}

#[tokio::test]
async fn cold_nonblocking_read_returns_none_and_starts_one_build() {
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let release_rx = Arc::new(StdMutex::new(Some(release_rx)));
    let build: SnapshotBuildFn = {
        let calls = calls.clone();
        let started = started.clone();
        let release_rx = release_rx.clone();
        Arc::new(move || {
            let calls = calls.clone();
            let started = started.clone();
            let release_rx = release_rx.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                started.notify_one();
                let gate = release_rx
                    .lock()
                    .expect("poisoned")
                    .take()
                    .expect("build gate taken once");
                let _released = gate.await;
                Ok(build_snapshot(&stub_input()))
            })
        })
    };
    let coordinator = SnapshotCoordinator::new(Duration::from_secs(3600), build);

    assert!(coordinator.latest_or_trigger_refresh().is_none());
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .expect("background build never started");
    assert!(coordinator.latest_or_trigger_refresh().is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let _released = release_tx.send(());
    let snapshot = tokio::time::timeout(Duration::from_secs(5), coordinator.get())
        .await
        .expect("background build never completed")
        .unwrap();
    let cached = coordinator
        .latest_or_trigger_refresh()
        .expect("completed build must be cached");
    assert!(Arc::ptr_eq(&snapshot, &cached));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn nonblocking_read_serves_fresh_cache_without_refreshing() {
    let calls = Arc::new(AtomicUsize::new(0));
    let coordinator = SnapshotCoordinator::new(
        Duration::from_secs(3600),
        counting_source(calls.clone(), Duration::ZERO),
    );

    let built = coordinator.get().await.unwrap();
    let cached = coordinator
        .latest_or_trigger_refresh()
        .expect("fresh cache must be returned");

    assert!(Arc::ptr_eq(&built, &cached));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(coordinator.state.lock().unwrap().flight.is_none());
}

#[tokio::test]
async fn stale_nonblocking_reads_share_one_refresh_and_keep_serving_old_cache() {
    let calls = Arc::new(AtomicUsize::new(0));
    let second_started = Arc::new(Notify::new());
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let release_rx = Arc::new(StdMutex::new(Some(release_rx)));
    let build: SnapshotBuildFn = {
        let calls = calls.clone();
        let second_started = second_started.clone();
        let release_rx = release_rx.clone();
        Arc::new(move || {
            let calls = calls.clone();
            let second_started = second_started.clone();
            let release_rx = release_rx.clone();
            Box::pin(async move {
                let attempt = calls.fetch_add(1, Ordering::SeqCst);
                if attempt == 1 {
                    second_started.notify_one();
                    let gate = release_rx
                        .lock()
                        .expect("poisoned")
                        .take()
                        .expect("refresh gate taken once");
                    let _released = gate.await;
                }
                Ok(build_snapshot(&stub_input()))
            })
        })
    };
    let coordinator = SnapshotCoordinator::new(Duration::ZERO, build);
    let old = coordinator.get().await.unwrap();

    let first_stale = coordinator
        .latest_or_trigger_refresh()
        .expect("stale cache remains available");
    assert!(Arc::ptr_eq(&old, &first_stale));
    tokio::time::timeout(Duration::from_secs(5), second_started.notified())
        .await
        .expect("refresh never started");

    for _ in 0..8 {
        let stale = coordinator
            .latest_or_trigger_refresh()
            .expect("all scrapes receive stale cache");
        assert!(Arc::ptr_eq(&old, &stale));
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "refresh must be single-flight"
    );

    let _released = release_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let finished = {
                let state = coordinator.state.lock().unwrap();
                state.flight.is_none()
                    && state
                        .cached
                        .as_ref()
                        .is_some_and(|cached| !Arc::ptr_eq(&old, &cached.payload))
            };
            if finished {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("background refresh never completed");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn dashboard_get_joins_refresh_claimed_by_nonblocking_read() {
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let release_rx = Arc::new(StdMutex::new(Some(release_rx)));
    let build: SnapshotBuildFn = {
        let calls = calls.clone();
        let started = started.clone();
        let release_rx = release_rx.clone();
        Arc::new(move || {
            let calls = calls.clone();
            let started = started.clone();
            let release_rx = release_rx.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                started.notify_one();
                let gate = release_rx
                    .lock()
                    .expect("poisoned")
                    .take()
                    .expect("build gate taken once");
                let _released = gate.await;
                Ok(build_snapshot(&stub_input()))
            })
        })
    };
    let coordinator = SnapshotCoordinator::new(Duration::from_secs(3600), build);

    assert!(coordinator.latest_or_trigger_refresh().is_none());
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .expect("background build never started");

    let mut dashboard = Box::pin(coordinator.get());
    let waker = futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    assert!(
        dashboard.as_mut().poll(&mut cx).is_pending(),
        "dashboard must wait for the claimed refresh"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let _released = release_tx.send(());
    let snapshot = tokio::time::timeout(Duration::from_secs(5), dashboard)
        .await
        .expect("dashboard waiter never received refresh")
        .unwrap();
    let cached = coordinator
        .latest_or_trigger_refresh()
        .expect("refresh result must be cached");
    assert!(Arc::ptr_eq(&snapshot, &cached));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_background_refresh_preserves_last_successful_cache() {
    let calls = Arc::new(AtomicUsize::new(0));
    let second_started = Arc::new(Notify::new());
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let release_rx = Arc::new(StdMutex::new(Some(release_rx)));
    let build: SnapshotArtifactsBuildFn<MetricsSnapshot> = {
        let calls = calls.clone();
        let second_started = second_started.clone();
        let release_rx = release_rx.clone();
        Arc::new(move || {
            let calls = calls.clone();
            let second_started = second_started.clone();
            let release_rx = release_rx.clone();
            Box::pin(async move {
                let attempt = calls.fetch_add(1, Ordering::SeqCst);
                if attempt == 1 {
                    second_started.notify_one();
                    let gate = release_rx
                        .lock()
                        .expect("poisoned")
                        .take()
                        .expect("failure gate taken once");
                    let _released = gate.await;
                    return Err("refresh failed".to_string());
                }
                Ok(stub_artifacts())
            })
        })
    };
    let coordinator = SnapshotCoordinator::new_with_artifacts(Duration::ZERO, build);
    let old = coordinator.get().await.unwrap();
    let old_metrics = coordinator
        .state
        .lock()
        .unwrap()
        .cached
        .as_ref()
        .and_then(|cached| cached.sidecar.clone())
        .expect("successful collection must cache metrics");

    let stale_metrics = coordinator
        .latest_sidecar_or_trigger_refresh()
        .expect("old metrics must remain available");
    assert!(Arc::ptr_eq(&old_metrics, &stale_metrics));
    tokio::time::timeout(Duration::from_secs(5), second_started.notified())
        .await
        .expect("failing refresh never started");

    let mut dashboard = Box::pin(coordinator.get());
    let waker = futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    assert!(dashboard.as_mut().poll(&mut cx).is_pending());
    let _released = release_tx.send(());
    let error = tokio::time::timeout(Duration::from_secs(5), dashboard)
        .await
        .expect("dashboard waiter never received refresh error")
        .unwrap_err();
    assert_eq!(error, "refresh failed");

    let state = coordinator.state.lock().unwrap();
    assert!(state.flight.is_none());
    assert!(
        state
            .cached
            .as_ref()
            .is_some_and(|cached| Arc::ptr_eq(&old, &cached.payload)),
        "failed refresh must not discard the last good snapshot"
    );
    assert!(
        state
            .cached
            .as_ref()
            .and_then(|cached| cached.sidecar.as_ref())
            .is_some_and(|metrics| Arc::ptr_eq(&old_metrics, metrics)),
        "failed refresh must not discard the last good metrics sidecar"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn dropping_unpolled_claimed_build_task_clears_only_its_flight() {
    let calls = Arc::new(AtomicUsize::new(0));
    let coordinator = SnapshotCoordinator::new(
        Duration::ZERO,
        counting_source(calls.clone(), Duration::ZERO),
    );
    let old = Arc::new(build_snapshot(&stub_input()));
    let flight = new_flight();
    {
        let mut state = coordinator.state.lock().unwrap();
        state.cached = Some(CachedSnapshot {
            payload: old.clone(),
            sidecar: None,
            built_at: Instant::now(),
        });
        state.flight = Some(flight.clone());
    }

    let task = coordinator.claimed_build_task(flight);
    drop(task);

    let state = coordinator.state.lock().unwrap();
    assert!(
        state.flight.is_none(),
        "dropped task must release its claim"
    );
    assert!(
        state
            .cached
            .as_ref()
            .is_some_and(|cached| Arc::ptr_eq(&old, &cached.payload)),
        "guard cleanup must preserve the last good cache"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0, "task was never polled");
}

#[tokio::test]
async fn cancelled_background_refresh_preserves_cached_metrics() {
    let started = Arc::new(Notify::new());
    let build: SnapshotArtifactsBuildFn<MetricsSnapshot> = {
        let started = started.clone();
        Arc::new(move || {
            let started = started.clone();
            Box::pin(async move {
                started.notify_one();
                std::future::pending::<Result<SnapshotArtifacts<MetricsSnapshot>, String>>().await
            })
        })
    };
    let coordinator = SnapshotCoordinator::new_with_artifacts(Duration::ZERO, build);
    let artifacts = stub_artifacts();
    let old_payload = Arc::new(artifacts.dashboard);
    let old_metrics = Arc::new(artifacts.sidecar.expect("stub metrics"));
    let flight = new_flight();
    {
        let mut state = coordinator.state.lock().unwrap();
        state.cached = Some(CachedSnapshot {
            payload: old_payload.clone(),
            sidecar: Some(old_metrics.clone()),
            built_at: Instant::now(),
        });
        state.flight = Some(flight.clone());
    }

    let refresh = tokio::spawn(coordinator.claimed_build_task(flight));
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .expect("background refresh never started");
    refresh.abort();
    assert!(refresh.await.unwrap_err().is_cancelled());

    let state = coordinator.state.lock().unwrap();
    assert!(state.flight.is_none());
    let cached = state.cached.as_ref().expect("old cache must remain");
    assert!(Arc::ptr_eq(&old_payload, &cached.payload));
    assert!(
        cached
            .sidecar
            .as_ref()
            .is_some_and(|metrics| Arc::ptr_eq(&old_metrics, metrics))
    );
}

#[tokio::test]
async fn panicked_background_refresh_preserves_cached_metrics() {
    let build: SnapshotArtifactsBuildFn<MetricsSnapshot> = Arc::new(|| {
        Box::pin(async move {
            panic!("simulated detached refresh panic");
        })
    });
    let coordinator = SnapshotCoordinator::new_with_artifacts(Duration::ZERO, build);
    let artifacts = stub_artifacts();
    let old_payload = Arc::new(artifacts.dashboard);
    let old_metrics = Arc::new(artifacts.sidecar.expect("stub metrics"));
    let flight = new_flight();
    {
        let mut state = coordinator.state.lock().unwrap();
        state.cached = Some(CachedSnapshot {
            payload: old_payload.clone(),
            sidecar: Some(old_metrics.clone()),
            built_at: Instant::now(),
        });
        state.flight = Some(flight.clone());
    }

    let join_error = tokio::spawn(coordinator.claimed_build_task(flight))
        .await
        .unwrap_err();
    assert!(join_error.is_panic());

    let state = coordinator.state.lock().unwrap();
    assert!(state.flight.is_none());
    let cached = state.cached.as_ref().expect("old cache must remain");
    assert!(Arc::ptr_eq(&old_payload, &cached.payload));
    assert!(
        cached
            .sidecar
            .as_ref()
            .is_some_and(|metrics| Arc::ptr_eq(&old_metrics, metrics))
    );
}

#[tokio::test]
async fn concurrent_waiters_share_one_build_and_late_result_is_delivered() {
    let calls = Arc::new(AtomicUsize::new(0));
    let coordinator = SnapshotCoordinator::new(
        Duration::from_secs(3600),
        counting_source(calls.clone(), Duration::from_millis(200)),
    );
    // Four getters race in while the single build is running; ALL waiters
    // get the completed result (F9: late results are never discarded).
    let mut join = Vec::new();
    for _ in 0..4 {
        let coordinator = coordinator.clone();
        join.push(tokio::spawn(async move { coordinator.get().await }));
    }
    let mut payloads = Vec::new();
    for handle in join {
        payloads.push(handle.await.unwrap().unwrap());
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "single-flight: exactly one build"
    );
    for payload in &payloads[1..] {
        assert!(Arc::ptr_eq(&payloads[0], payload));
    }
}

#[tokio::test]
async fn build_errors_reach_every_waiter_and_are_never_cached() {
    let calls = Arc::new(AtomicUsize::new(0));
    let fail = calls.clone();
    let build: SnapshotBuildFn = Arc::new(move || {
        let fail = fail.clone();
        Box::pin(async move {
            let attempt = fail.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            if attempt == 0 {
                Err("boom".to_string())
            } else {
                Ok(build_snapshot(&stub_input()))
            }
        })
    });
    let coordinator = SnapshotCoordinator::new(Duration::from_secs(3600), build);
    let first = coordinator.get().await;
    assert!(matches!(&first, Err(message) if message == "boom"));
    // Next call rebuilds instead of replaying the error.
    let second = coordinator.get().await;
    assert!(second.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

/// Deterministic error fan-out: ALL waiters that joined one build
/// generation must receive THAT generation's error — identical to the
/// builder's — with NO duplicate rebuild. Driven by manual polls (no
/// sleeps, no scheduler dependence): the builder future is polled once to
/// park inside the blocking attempt-1 build, then each waiter is polled
/// once to join the same flight (a parked manual poll proves it joined —
/// the enable completes synchronously inside that poll), attempt 1 is
/// released to `Err("boom")` through a oneshot gate, and every parked
/// waiter must resolve to the same "boom" on its very next poll.
#[tokio::test]
async fn build_error_fans_out_to_all_waiters_of_the_same_generation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let build: SnapshotBuildFn = {
        let calls = calls.clone();
        let release_rx = Arc::new(StdMutex::new(Some(release_rx)));
        Arc::new(move || {
            let calls = calls.clone();
            let release_rx = release_rx.clone();
            Box::pin(async move {
                let attempt = calls.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    // Park until the test releases this generation to its error.
                    let gate = release_rx.lock().expect("poisoned").take();
                    let gate = gate.expect("attempt-1 gate released only once");
                    // Best-effort gate await; either outcome releases the parked build.
                    let _released = gate.await;
                    Err("boom".to_string())
                } else {
                    Ok(build_snapshot(&stub_input()))
                }
            })
        })
    };
    let coordinator = SnapshotCoordinator::new(Duration::from_secs(3600), build);

    // Attempt 1: one manual poll claims the builder role synchronously and
    // parks inside the blocking build.
    let mut builder = Box::pin(coordinator.get());
    let waker = futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    assert!(
        builder.as_mut().poll(&mut cx).is_pending(),
        "attempt 1 must be parked inside the blocking build"
    );

    // N waiters join the SAME generation: each first manual poll observes
    // a flight, registers AND enables on it under the decision
    // guard, then parks — all in that single poll (deterministic seam,
    // no sleeps/yields).
    const N: usize = 4;
    let mut waiters: Vec<_> = (0..N)
        .map(|_| {
            let mut waiter = Box::pin(coordinator.get());
            assert!(
                waiter.as_mut().poll(&mut cx).is_pending(),
                "waiter must join and park on the in-flight generation"
            );
            waiter
        })
        .collect();

    // Release attempt 1 to its error and drive the builder to completion.
    // Best-effort release; the receiver is dropped if the build already errored.
    let _released = release_tx.send(());
    let std::task::Poll::Ready(Err(message)) = builder.as_mut().poll(&mut cx) else {
        panic!("builder must resolve with the attempt-1 error");
    };
    assert_eq!(message, "boom");

    // Every joined waiter fans out to the same stored error — the enabled
    // notification was already fired, so each waiter resolves immediately
    // on its next poll, and crucially builds NOTHING new.
    for waiter in waiters.iter_mut() {
        match waiter.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(Err(message)) => assert_eq!(message, "boom"),
            _ => panic!("waiter must receive the same generation error"),
        }
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "error fan-out must not trigger a duplicate rebuild"
    );

    // Only a LATER get() retries: attempt 2 builds fresh and succeeds.
    let retry = tokio::time::timeout(Duration::from_secs(5), coordinator.get())
        .await
        .expect("retry build never resolved within 5s")
        .unwrap();
    assert_eq!(retry.schema_version, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn waiter_arriving_mid_build_gets_same_result_not_duplicate_work() {
    let calls = Arc::new(AtomicUsize::new(0));
    let coordinator = SnapshotCoordinator::new(
        Duration::from_secs(3600),
        counting_source(calls.clone(), Duration::from_millis(300)),
    );
    let first = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.get().await })
    };
    // Let the first caller settle into the builder role, then pile on.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let second = coordinator.get().await;
    let first = first.await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(second.is_ok(), first.is_ok());
}

#[test]
fn coordinator_is_clone_cheap() {
    let coordinator = SnapshotCoordinator::new(
        Duration::from_secs(1),
        counting_source(Arc::new(AtomicUsize::new(0)), Duration::ZERO),
    );
    let clone = coordinator.clone();
    assert_eq!(clone.ttl, coordinator.ttl);
}

// ── F1 lost-wakeup / cancellation / panic regressions ───────────────────

/// Deterministic lost-wakeup regression: completion is forced into the
/// exact window between the waiter's decision poll and its await poll,
/// with zero scheduler races — the test holds the state lock and the
/// wakers directly. The waiter's registration MUST be bound to the same
/// decision guard that observed an in-flight build (not deferred past an
/// unlock): `notify_waiters` only reaches already-registered waiters, so
/// any registration that happens after the decision guard dropped would
/// miss this completion and hang forever (the timeout catches it).
#[tokio::test]
async fn completion_in_decision_window_sets_waiter_notified() {
    let coordinator = SnapshotCoordinator::new(
        Duration::from_secs(3600),
        counting_source(Arc::new(AtomicUsize::new(0)), Duration::ZERO),
    );
    // Install a flight exactly as a real in-flight build would.
    let flight = new_flight();
    coordinator
        .state
        .lock()
        .expect("coordinator poisoned")
        .flight = Some(flight.clone());

    // First poll: decision observes the flight and must register+enable the
    // waiter UNDER the decision guard, before the guard is released.
    let mut waiter = Box::pin(coordinator.get());
    let waker = futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    assert!(
        waiter.as_mut().poll(&mut cx).is_pending(),
        "waiter must park on the in-flight build"
    );

    // The build completes in the window after the waiter's decision,
    // mirroring the real builder: store the flight outcome, publish the
    // cache and clear the flight, then notify while the waiter is NOT being
    // polled. A waiter whose registration depends on a later lock
    // acquisition would sleep through this wakeup forever.
    let payload = Arc::new(build_snapshot(&stub_input()));
    *flight.outcome.lock().expect("coordinator poisoned") = Some(Ok(payload.clone()));
    {
        let mut state = coordinator.state.lock().expect("coordinator poisoned");
        state.cached = Some(CachedSnapshot {
            payload: payload.clone(),
            sidecar: None,
            built_at: Instant::now(),
        });
        state.flight = None;
    }
    flight.notify.notify_waiters();

    // The waiter wakes from the enabled registration and returns the
    // flight's stored outcome; the window-resident completion is not lost.
    let served = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .expect("lost wakeup: waiter hung on a completed build")
        .unwrap();
    assert!(Arc::ptr_eq(&payload, &served));
}

/// A waiter that observes an in-flight build must register its `Notified`
/// before the lock drops, so a builder completing the instant the waiter
/// unlocks cannot lose the wakeup. Bounding the whole join by a timeout
/// turns a regression (a waiter hanging on a `Notify` that already fired)
/// into a fast test failure instead of an infinite hang.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn waiters_racing_completion_never_lose_the_wakeup() {
    for round in 0..25 {
        let calls = Arc::new(AtomicUsize::new(0));
        let coordinator = SnapshotCoordinator::new(
            Duration::from_secs(3600),
            counting_source(calls.clone(), Duration::from_millis(2)),
        );
        // Builder starts first, then waiters pile on around completion.
        let mut join = Vec::new();
        join.push({
            let coordinator = coordinator.clone();
            tokio::spawn(async move { coordinator.get().await })
        });
        tokio::time::sleep(Duration::from_millis(1)).await;
        for _ in 0..8 {
            let coordinator = coordinator.clone();
            join.push(tokio::spawn(async move { coordinator.get().await }));
        }
        let payloads = tokio::time::timeout(Duration::from_secs(5), async {
            let mut out = Vec::new();
            for handle in join {
                out.push(handle.await.unwrap().unwrap());
            }
            out
        })
        .await
        .expect("lost wakeup: a waiter never resolved within 5s");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "single-flight preserved across round {round}"
        );
        for payload in &payloads[1..] {
            assert!(Arc::ptr_eq(&payloads[0], payload));
        }
    }
}

/// When the builder task is cancelled mid-build, the stranded flight must
/// be cleared and wake any waiters, so a later
/// `get()` starts a fresh build instead of hanging on a dead `Notify`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_builder_resets_slot_and_wakes_waiters() {
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(tokio::sync::Notify::new());
    let build: SnapshotBuildFn = {
        let calls = calls.clone();
        let started = started.clone();
        Arc::new(move || {
            let calls = calls.clone();
            let started = started.clone();
            Box::pin(async move {
                let attempt = calls.fetch_add(1, Ordering::SeqCst);
                started.notify_waiters();
                if attempt == 0 {
                    // Never resolves — only cancellation ends this build.
                    std::future::pending::<Result<_, String>>().await
                } else {
                    Ok(build_snapshot(&stub_input()))
                }
            })
        })
    };
    let coordinator = SnapshotCoordinator::new(Duration::from_secs(3600), build);

    let builder = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.get().await })
    };
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .expect("builder never started within 5s");

    // A waiter parked on the in-flight build.
    let waiter = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.get().await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;

    builder.abort(); // cancel the builder mid-build
    // Reap the aborted builder task; its result is intentionally discarded.
    let _aborted = builder.await;

    // The waiter and a fresh caller both complete via a rebuilt (attempt 2).
    let fresh = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.get().await })
    };
    let outcomes = tokio::time::timeout(Duration::from_secs(5), async {
        [waiter.await.unwrap(), fresh.await.unwrap()]
    })
    .await
    .expect("stranded flight: a get() never resolved within 5s");
    assert!(outcomes[0].is_ok());
    assert!(outcomes[1].is_ok());
    assert!(
        calls.load(Ordering::SeqCst) >= 2,
        "cancelled build did not deliver; a fresh build ran"
    );
}

/// A build that panics must clear its flight and wake waiters, so the next
/// `get()` rebuilds instead of hanging on the panicked build's `Notify`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panicked_builder_resets_slot_and_wakes_waiters() {
    let calls = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(tokio::sync::Notify::new());
    let build: SnapshotBuildFn = {
        let calls = calls.clone();
        let started = started.clone();
        Arc::new(move || {
            let calls = calls.clone();
            let started = started.clone();
            Box::pin(async move {
                let attempt = calls.fetch_add(1, Ordering::SeqCst);
                started.notify_waiters();
                if attempt == 0 {
                    panic!("simulated build failure");
                }
                Ok(build_snapshot(&stub_input()))
            })
        })
    };
    let coordinator = SnapshotCoordinator::new(Duration::from_secs(3600), build);

    // The panicking build runs in a spawned task so the test body survives.
    let builder = {
        let coordinator = coordinator.clone();
        tokio::spawn(async move { coordinator.get().await })
    };
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .expect("builder never started within 5s");

    // The panic (caught by the spawned task) clears the flight.
    let join_err = builder.await.unwrap_err();
    assert!(join_err.is_panic(), "expected the build to panic");

    // A later get() rebuilds fresh and succeeds within a timeout.
    let retry = tokio::time::timeout(Duration::from_secs(5), coordinator.get())
        .await
        .expect("stranded flight: retry never resolved within 5s");
    assert!(retry.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
