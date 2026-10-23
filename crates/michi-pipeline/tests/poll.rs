//! The poll step: one attempt that checks on a fixed schedule until ready, a definitive failure, its deadline or
//! cancellation. All on Tokio's paused clock.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use michi_pipeline::{
    execute, poll_step, AttemptBound, CancellationToken, Effect, ExecOptions, ExecStep, ExecutionError, PollSchedule,
    Readiness, Replay, RetryBudget, RunOutcome, SkipReason, Step, StepStatus,
};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// What a scripted check answers, by check number (the last repeats).
#[derive(Clone)]
enum Answer {
    Ready,
    NotYet(&'static str),
    Err(ExecutionError),
    Hang,
}

/// Check times in ms since the test started.
#[derive(Clone, Default)]
struct Checks(Arc<Mutex<Vec<u64>>>);

impl Checks {
    fn times(&self) -> Vec<u64> {
        self.0.lock().unwrap().clone()
    }
}

/// A poll step whose n-th check answers `answers[n]`, logging each check's time.
fn poller(id: &str, schedule: PollSchedule, answers: Vec<Answer>, checks: &Checks) -> ExecStep {
    let start = tokio::time::Instant::now();
    let checks = checks.clone();
    ExecStep::new(
        sid(id),
        format!("Poll {id}"),
        poll_step(schedule, move |_ctx| {
            let n = {
                let mut times = checks.0.lock().unwrap();
                times.push(u64::try_from(start.elapsed().as_millis()).unwrap());
                times.len() - 1
            };
            let answer = answers[n.min(answers.len() - 1)].clone();
            async move {
                match answer {
                    Answer::Ready => Ok(Readiness::Ready),
                    Answer::NotYet(seen) => Ok(Readiness::NotYet(seen.to_string())),
                    Answer::Err(e) => Err(e),
                    Answer::Hang => std::future::pending().await,
                }
            }
        }),
    )
}

#[tokio::test(start_paused = true)]
async fn the_default_schedule_checks_at_once_then_after_2_4_8_16_then_every_30_seconds() {
    let checks = Checks::default();
    let mut answers = vec![Answer::NotYet("1.2.2"); 6];
    answers.push(Answer::Ready);
    let p = plan(vec![poller("verify", PollSchedule::default(), answers, &checks).deadline(secs(600))]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(checks.times(), [0, 2_000, 6_000, 14_000, 30_000, 60_000, 90_000]);
    assert_eq!(record.step(&sid("verify")).unwrap().attempts().len(), 1, "the whole poll is one attempt");
}

#[tokio::test(start_paused = true)]
async fn a_custom_schedule_is_followed() {
    let checks = Checks::default();
    let schedule = PollSchedule::new([ms(100), ms(200)], ms(50));
    let answers =
        vec![Answer::NotYet("no"), Answer::NotYet("no"), Answer::NotYet("no"), Answer::NotYet("no"), Answer::Ready];
    let p = plan(vec![poller("verify", schedule, answers, &checks).deadline(secs(10))]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(checks.times(), [0, 100, 300, 350, 400]);
}

#[tokio::test(start_paused = true)]
async fn polling_stops_when_the_next_check_would_fall_past_the_group_deadline() {
    let checks = Checks::default();
    let p =
        plan(vec![poller("verify", PollSchedule::default(), vec![Answer::NotYet("registry serves 1.2.2")], &checks)
            .deadline_group("verify")]);
    let opts = ExecOptions::new().group_deadline("verify", secs(20));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(checks.times(), [0, 2_000, 6_000, 14_000], "the next check (30 s) would be past the 20 s window");
    let step = record.step(&sid("verify")).unwrap();
    assert_eq!(step.status(), StepStatus::Failed);
    assert_eq!(step.attempts().len(), 1, "never retried");
    assert_eq!(step.elapsed(), Some(secs(14)));
    let err = step.error().unwrap();
    assert!(matches!(err, ExecutionError::DeadlinePassed { .. }), "{err:?}");
    assert_eq!(err.effect(), Effect::NotApplied, "a poll only reads");
    let text = err.to_string();
    assert!(text.contains("4 checks") && text.contains("last observed: registry serves 1.2.2"), "{text}");
    assert!(record.needs_verification().next().is_none());
}

#[tokio::test(start_paused = true)]
async fn the_attempt_timeout_bounds_each_check_not_the_whole_poll() {
    let checks = Checks::default();
    let answers = vec![Answer::Hang, Answer::NotYet("no"), Answer::NotYet("no"), Answer::Ready];
    let p = plan(vec![poller("verify", PollSchedule::default(), answers, &checks).deadline(secs(60))]);
    let opts = ExecOptions::new().attempt_timeout(secs(1));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded, "the poll ran past the 1 s attempt timeout");
    // The first check hangs and is cut at 1 s, then the schedule continues from there.
    assert_eq!(checks.times(), [0, 3_000, 7_000, 15_000]);
}

#[tokio::test(start_paused = true)]
async fn a_check_still_running_at_the_deadline_ends_the_poll_with_the_poll_error_not_a_timeout() {
    let checks = Checks::default();
    let p = plan(vec![poller("verify", PollSchedule::default(), vec![Answer::Hang], &checks).deadline(secs(5))]);
    let opts = ExecOptions::new().attempt_timeout(secs(60));
    let record = execute(&p, rid("r"), &opts).await;
    let step = record.step(&sid("verify")).unwrap();
    assert_eq!(step.elapsed(), Some(secs(5)));
    let err = step.error().unwrap();
    assert!(matches!(err, ExecutionError::DeadlinePassed { .. }), "{err:?}");
    assert!(err.to_string().contains("last observed: the check timed out after 5000ms"), "{err}");
}

#[tokio::test(start_paused = true)]
async fn without_a_deadline_the_attempt_timeout_bounds_the_whole_poll() {
    let checks = Checks::default();
    let p = plan(vec![poller("verify", PollSchedule::default(), vec![Answer::NotYet("no")], &checks)]);
    let opts = ExecOptions::new().attempt_timeout(secs(10));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(checks.times(), [0, 2_000, 6_000]);
    let step = record.step(&sid("verify")).unwrap();
    assert_eq!(step.status(), StepStatus::Failed);
    assert!(matches!(step.error(), Some(ExecutionError::DeadlinePassed { .. })), "{:?}", step.error());
    assert_eq!(step.attempts().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_definitive_check_failure_ends_the_poll() {
    let checks = Checks::default();
    let answers = vec![Answer::NotYet("no"), Answer::Err(ExecutionError::refused("version was unpublished"))];
    let p = plan(vec![poller("verify", PollSchedule::default(), answers, &checks).deadline(secs(60))]);
    let record = execute(&p, rid("r"), &ExecOptions::new()).await;
    assert_eq!(checks.times(), [0, 2_000]);
    let step = record.step(&sid("verify")).unwrap();
    assert_eq!(step.status(), StepStatus::Failed);
    assert_eq!(step.error(), Some(&ExecutionError::refused("version was unpublished")));
}

#[tokio::test(start_paused = true)]
async fn retryable_check_failures_keep_polling_and_never_draw_from_the_retry_budget() {
    // A budget with exactly one retry in it, and a poll whose checks fail retryably four times. Afterwards `flaky`
    // needs one retry: it gets it, so the poll took nothing from the budget.
    let checks = Checks::default();
    let log = Log::new();
    let mut answers = vec![Answer::Err(ExecutionError::rejected("503 from registry")); 4];
    answers.push(Answer::Ready);
    let p = plan(vec![
        poller("verify", PollSchedule::default(), answers, &checks).deadline(secs(600)).retry(retries(5, 10)),
        exec(
            "flaky",
            &["verify"],
            Scripted::new(&log, "flaky", vec![Act::Err(ExecutionError::rejected("busy")), Act::Ok]),
        )
        .retry(retries(1, 10)),
    ]);
    let opts = ExecOptions::new().retry_budget(RetryBudget::new(0.0, 1, 1));
    let record = execute(&p, rid("r"), &opts).await;
    assert_eq!(record.outcome(), RunOutcome::Succeeded);
    assert_eq!(checks.times().len(), 5);
    assert_eq!(record.step(&sid("verify")).unwrap().attempts().len(), 1);
    assert_eq!(record.step(&sid("flaky")).unwrap().attempts().len(), 2, "the one budgeted retry was still there");
}

#[tokio::test(start_paused = true)]
async fn cancellation_stops_the_poll_at_once() {
    let checks = Checks::default();
    let p =
        plan(vec![poller("verify", PollSchedule::default(), vec![Answer::NotYet("no")], &checks).deadline(secs(600))]);
    let token = CancellationToken::new();
    let opts = ExecOptions::new().cancellation(token.clone());
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(secs(5)).await;
        token.cancel();
    });
    let record = execute(&p, rid("r"), &opts).await;
    canceller.await.unwrap();
    assert_eq!(checks.times(), [0, 2_000]);
    assert_eq!(status(&record, "verify"), StepStatus::Skipped(SkipReason::Cancelled));
    assert_eq!(record.elapsed(), secs(5), "the poll stopped when the token fired, not after the grace period");
}

#[tokio::test(start_paused = true)]
async fn cancellation_during_a_check_stops_the_poll_too() {
    let checks = Checks::default();
    let p = plan(vec![poller("verify", PollSchedule::default(), vec![Answer::Hang], &checks).deadline(secs(600))]);
    let token = CancellationToken::new();
    let opts = ExecOptions::new().cancellation(token.clone()).attempt_timeout(secs(60));
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(secs(5)).await;
        token.cancel();
    });
    let record = execute(&p, rid("r"), &opts).await;
    canceller.await.unwrap();
    assert_eq!(status(&record, "verify"), StepStatus::Skipped(SkipReason::Cancelled));
    assert_eq!(record.elapsed(), secs(5));
}

#[test]
fn a_poll_step_is_idempotent_and_bounded_by_its_deadline() {
    let step = poll_step(PollSchedule::default(), |_ctx| async { Ok(Readiness::Ready) });
    assert_eq!(step.replay(), Replay::Idempotent);
    assert_eq!(step.attempt_bound(), AttemptBound::Deadline);
}

#[test]
fn the_schedule_lists_its_delays_and_floors_zero() {
    let default = PollSchedule::default();
    let delays: Vec<Duration> = (0..7).map(|n| default.delay_after(n)).collect();
    assert_eq!(delays, [secs(2), secs(4), secs(8), secs(16), secs(30), secs(30), secs(30)]);
    let zero = PollSchedule::new([Duration::ZERO], Duration::ZERO);
    assert_eq!((zero.delay_after(0), zero.delay_after(5)), (ms(1), ms(1)), "a zero delay would spin");
    assert_eq!(PollSchedule::every(secs(5)).delay_after(3), secs(5));
}
