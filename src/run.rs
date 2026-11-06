use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::http::{Conn, Target};
use crate::stats::Summary;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Requests go out on a fixed schedule whatever the server does, and latency counts from
    /// when each was due. A stall shows up in every request that should have gone out during it.
    Open,
    /// Each connection waits for its response before its next request, skips the slots it
    /// missed meanwhile, and counts latency from when it actually sent. This is how many load
    /// generators work, and it under-reports stalls.
    Closed,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub target: Target,
    /// Requests per second, across all connections.
    pub rate: f64,
    pub duration: Duration,
    pub connections: usize,
    pub mode: Mode,
    pub timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub mode: Mode,
    pub latency: Summary,
    /// Requests the schedule called for.
    pub scheduled: u64,
    /// Requests that got a response.
    pub completed: u64,
    pub errors: u64,
    pub elapsed: Duration,
}

/// Runs a plan and reports what it measured.
pub fn run(plan: &Plan) -> Report {
    assert!(plan.rate > 0.0 && plan.connections > 0);
    let total = (plan.rate * plan.duration.as_secs_f64()).round() as u64;
    // A moment to open the connections before the first request is due.
    let start = Instant::now() + Duration::from_millis(100);
    let next = Arc::new(AtomicU64::new(0));
    let workers: Vec<_> = (0..plan.connections)
        .map(|worker| {
            let (plan, next) = (plan.clone(), next.clone());
            thread::spawn(move || work(&plan, worker, start, total, &next))
        })
        .collect();
    let mut latencies = Vec::new();
    let mut errors = 0;
    for worker in workers {
        let (mut l, e) = worker.join().expect("a worker panicked");
        latencies.append(&mut l);
        errors += e;
    }
    Report {
        mode: plan.mode,
        completed: latencies.len() as u64,
        latency: Summary::of(latencies),
        scheduled: total,
        errors,
        elapsed: start.elapsed(),
    }
}

/// The time request `k` is due, at `rate` requests a second from `start`.
fn due(start: Instant, k: u64, rate: f64) -> Instant {
    start + Duration::from_secs_f64(k as f64 / rate)
}

fn work(
    plan: &Plan,
    worker: usize,
    start: Instant,
    total: u64,
    next: &AtomicU64,
) -> (Vec<u64>, u64) {
    let mut latencies = Vec::new();
    let mut errors = 0;
    let mut conn: Option<Conn> = None;
    let mut send = |conn: &mut Option<Conn>| -> bool {
        if conn.as_ref().is_none_or(|c| !c.reusable) {
            *conn = Conn::connect(&plan.target, plan.timeout).ok();
        }
        let ok = conn.as_mut().is_some_and(|c| c.get().is_ok());
        if !ok {
            *conn = None;
            errors += 1;
        }
        ok
    };
    match plan.mode {
        Mode::Open => loop {
            // Whichever connection is free takes the next request due, so requests due while
            // every connection is busy wait, and that wait counts.
            let k = next.fetch_add(1, Ordering::Relaxed);
            if k >= total {
                break;
            }
            let at = due(start, k, plan.rate);
            sleep_until(at);
            if send(&mut conn) {
                latencies.push(at.elapsed().as_micros() as u64);
            }
        },
        Mode::Closed => {
            // Connection `w` takes slots w, w + n, w + 2n… of the same schedule.
            let (n, w) = (plan.connections as u64, worker as u64);
            let mut k = 0;
            while k * n + w < total {
                sleep_until(due(start, k * n + w, plan.rate));
                let sent = Instant::now();
                if send(&mut conn) {
                    latencies.push(sent.elapsed().as_micros() as u64);
                }
                // Its slots that went by while it waited are skipped, never sent.
                let now = start.elapsed().as_secs_f64() * plan.rate;
                let caught_up = ((now - w as f64) / n as f64).ceil().max(0.0) as u64;
                k = (k + 1).max(caught_up);
            }
        }
    }
    (latencies, errors)
}

fn sleep_until(at: Instant) {
    let now = Instant::now();
    if at > now {
        thread::sleep(at - now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schedule_is_evenly_spaced() {
        let start = Instant::now();
        assert_eq!(due(start, 0, 100.0), start);
        assert_eq!(due(start, 50, 100.0) - start, Duration::from_millis(500));
        assert_eq!(due(start, 3, 2.0) - start, Duration::from_millis(1500));
    }
}
