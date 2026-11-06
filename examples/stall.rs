//! cargo run --release --example stall
//!
//! A server that answers in 1 ms, except for one second when it answers nothing. The same plan,
//! measured both ways.

use std::time::{Duration, Instant};

use rs_loadgen::http::Target;
use rs_loadgen::server::{Behavior, spawn};
use rs_loadgen::{Mode, Plan, run};

fn main() {
    for mode in [Mode::Closed, Mode::Open] {
        // The run starts about 0.1 s from now; the stall covers its second second.
        let stall = (
            Instant::now() + Duration::from_millis(1100),
            Duration::from_secs(1),
        );
        let addr = spawn(Behavior {
            service: Duration::from_millis(1),
            stall: Some(stall),
        })
        .unwrap();
        let plan = Plan {
            target: Target::parse(&format!("http://{addr}/")).unwrap(),
            rate: 500.0,
            duration: Duration::from_secs(5),
            connections: 50,
            mode,
            timeout: Duration::from_secs(5),
        };
        let r = run(&plan);
        let l = &r.latency;
        let ms = |us: u64| format!("{:.1}", us as f64 / 1000.0);
        println!(
            "| {:?} | {} of {} | {} | {} | {} | {} | {} |",
            mode,
            r.completed,
            r.scheduled,
            ms(l.p50),
            ms(l.p90),
            ms(l.p99),
            ms(l.p999),
            ms(l.max)
        );
    }
}
