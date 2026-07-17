//! G4 cancellation conformance at the fs-exec -> FrankenSQLite boundary.

use std::sync::mpsc;
use std::time::Duration;

use fs_exec::{Budget, LatencyLane};
use fsqlite::{AsyncConnection, Connection, FrankenError};

const MARKER_POLL_QUOTA: u32 = 10_000;
const MARKER_COST_QUOTA: u64 = 7_919;
const MARKER_PRIORITY: u8 = 211;

#[test]
fn latency_lane_ambient_context_reaches_fsqlite_waiters() {
    // Take a local FrankenSQLite context from outside any asupersync task. It
    // deliberately has no attached native context, so the async facade can
    // proceed only by observing the latency task's ambient native Cx.
    let seed = Connection::open(":memory:").expect("create detached FrankenSQLite context");
    let local_cx = seed.root_cx().clone();
    assert!(
        local_cx.attached_native_cx().is_none(),
        "proof requires a detached local context"
    );
    let lane = LatencyLane::new(1).expect("build latency lane");
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let runtime_handle = lane.runtime().handle();
    let blocking_pool = runtime_handle
        .blocking_handle()
        .expect("latency lane must expose its blocking pool");

    runtime_handle.spawn_with_cx(move |root_cx| async move {
        let budget = Budget::new()
            .with_poll_quota(MARKER_POLL_QUOTA)
            .with_cost_quota(MARKER_COST_QUOTA)
            .with_priority(MARKER_PRIORITY);
        let scope = root_cx.scope_with_budget(budget);

        let outcome = match root_cx.spawn_in(&scope, move |child_cx| async move {
            let observed = child_cx.budget();
            if observed.poll_quota != MARKER_POLL_QUOTA
                || observed.cost_quota != Some(MARKER_COST_QUOTA)
                || observed.priority != MARKER_PRIORITY
            {
                return Err(format!("spawned child lost marker budget: {observed:?}"));
            }

            let mut connection = AsyncConnection::open(&local_cx, ":memory:")
                .await
                .map_err(|error| format!("ambient open failed: {error}"))?;
            connection
                .execute_batch(
                    &local_cx,
                    "CREATE TABLE ambient_probe(value INTEGER NOT NULL);\
                         INSERT INTO ambient_probe VALUES (42);",
                )
                .await
                .map_err(|error| format!("ambient setup failed: {error}"))?;

            let rows = connection
                .query(&local_cx, "SELECT value FROM ambient_probe")
                .await
                .map_err(|error| format!("ambient query failed: {error}"))?;
            if rows.len() != 1 {
                return Err(format!(
                    "ambient query returned {} rows instead of one",
                    rows.len()
                ));
            }

            // Keep the second blocking slot occupied so the response
            // waiter cannot publish the worker's value before the
            // cancelled Cx is polled. The already-dispatched statement is
            // read-only; this assertion is about response-wait
            // cancellation, not preemption or rollback.
            let (started_tx, started_rx) = mpsc::sync_channel(1);
            let (release_tx, release_rx) = mpsc::channel();
            let blocker = blocking_pool.spawn(move || {
                let _ = started_tx.send(());
                // Self-release bounds the failure path if ambient
                // cancellation regresses and the query remains pending.
                let _ = release_rx.recv_timeout(Duration::from_secs(5));
            });
            if started_rx.recv_timeout(Duration::from_secs(5)).is_err() {
                let _ = release_tx.send(());
                let _ = blocker.wait_timeout(Duration::from_secs(5));
                return Err("second blocking slot did not start its proof gate".into());
            }

            child_cx.set_cancel_requested(true);
            let local_stayed_healthy = local_cx.checkpoint().is_ok();
            let cancelled_query = connection
                .query(&local_cx, "SELECT value FROM ambient_probe")
                .await;
            child_cx.set_cancel_requested(false);
            let _ = release_tx.send(());
            if !blocker.wait_timeout(Duration::from_secs(5)) {
                return Err("proof gate did not drain after release".into());
            }
            if !local_stayed_healthy {
                return Err("detached FrankenSQLite context was unexpectedly cancelled".into());
            }
            match cancelled_query {
                Err(FrankenError::Interrupt) => {}
                Err(error) => {
                    return Err(format!(
                        "ambient cancellation returned {error}, expected Interrupt"
                    ));
                }
                Ok(_) => {
                    return Err("ambient cancellation did not interrupt the database wait".into());
                }
            }

            connection
                .query(&local_cx, "SELECT value FROM ambient_probe")
                .await
                .map_err(|error| format!("query did not recover after cancellation: {error}"))?;
            connection
                .close(&local_cx)
                .await
                .map_err(|error| format!("connection close failed: {error}"))?;
            Ok(())
        }) {
            Ok(mut child) => match child.join(&root_cx).await {
                Ok(outcome) => outcome,
                Err(error) => Err(format!("ambient child failed to join: {error:?}")),
            },
            Err(error) => Err(format!("ambient child failed to spawn: {error:?}")),
        };

        let _ = result_tx.send(outcome);
    });

    let outcome = result_rx
        .recv_timeout(Duration::from_secs(15))
        .expect("ambient proof task did not finish within 15 seconds");
    drop(seed);
    if let Err(error) = outcome {
        panic!("ambient context proof failed: {error}");
    }
}
