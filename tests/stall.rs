//! The measurement against a server that stalls for a known second, so the true latencies are
//! known: requests due during the stall wait until it ends.

use std::time::{Duration, Instant};

use rs_loadgen::http::Target;
use rs_loadgen::server::{Behavior, spawn};
use rs_loadgen::{Mode, Plan, Report, run};

/// 3 seconds at 200 requests a second, with the server stalled for the middle one.
fn stalled(mode: Mode) -> Report {
    let stall = (
        Instant::now() + Duration::from_millis(1100),
        Duration::from_secs(1),
    );
    let addr = spawn(Behavior {
        service: Duration::from_millis(1),
        stall: Some(stall),
    })
    .unwrap();
    run(&Plan {
        target: Target::parse(&format!("http://{addr}/")).unwrap(),
        rate: 200.0,
        duration: Duration::from_secs(3),
        connections: 20,
        mode,
        timeout: Duration::from_secs(5),
    })
}

const MS: u64 = 1000;

#[test]
fn the_open_model_sees_the_stall_in_every_request_it_delayed() {
    let r = stalled(Mode::Open);
    // 200 of the 600 requests were due during the stall, their latencies spread from 1 s down to
    // nothing: the slowest 60, a tenth of all, waited at least 0.7 s.
    assert_eq!((r.completed, r.scheduled, r.errors), (600, 600, 0));
    assert!(
        (500 * MS..900 * MS).contains(&r.latency.p90),
        "p90 {}",
        r.latency
    );
    assert!(r.latency.p50 < 100 * MS, "p50 {}", r.latency);
    assert!(
        r.latency.max > 900 * MS && r.latency.max < 1300 * MS,
        "max {}",
        r.latency
    );
}

#[test]
fn the_closed_model_hides_it() {
    let r = stalled(Mode::Closed);
    // Each of the 20 connections had one request held up; the rest due during the stall were
    // never sent, so most of the slow requests are missing from its numbers.
    assert!(r.completed < 500, "{} of {} sent", r.completed, r.scheduled);
    assert!(r.latency.p90 < 100 * MS, "p90 {}", r.latency);
    assert!(r.latency.max > 900 * MS, "max {}", r.latency);
}

#[test]
fn without_a_stall_both_agree() {
    let addr = spawn(Behavior {
        service: Duration::from_millis(1),
        stall: None,
    })
    .unwrap();
    for mode in [Mode::Open, Mode::Closed] {
        let r = run(&Plan {
            target: Target::parse(&format!("http://{addr}/")).unwrap(),
            rate: 200.0,
            duration: Duration::from_secs(1),
            connections: 10,
            mode,
            timeout: Duration::from_secs(5),
        });
        assert_eq!((r.completed, r.errors), (200, 0), "{mode:?}");
        assert!(r.latency.p99 < 100 * MS, "{mode:?} {}", r.latency);
    }
}

#[test]
fn an_unreachable_server_counts_errors_instead_of_hanging() {
    // Bind and drop a listener to get a port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let started = Instant::now();
    let r = run(&Plan {
        target: Target::parse(&format!("http://127.0.0.1:{port}/")).unwrap(),
        rate: 50.0,
        duration: Duration::from_secs(1),
        connections: 5,
        mode: Mode::Open,
        timeout: Duration::from_millis(200),
    });
    assert_eq!((r.completed, r.errors), (0, 50));
    // Linux refuses at once; Windows retries until the connect timeout. Either way, 10 rounds of
    // at most 0.2 s per connection, not a hang.
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_server_that_closes_after_each_response_gets_a_new_connection_each_time() {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 0) && line != "\r\n" {
                line.clear();
            }
            let mut stream = stream;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\nok");
        }
    });
    let r = run(&Plan {
        target: Target::parse(&format!("http://{addr}/")).unwrap(),
        rate: 50.0,
        duration: Duration::from_millis(400),
        connections: 1,
        mode: Mode::Open,
        timeout: Duration::from_secs(2),
    });
    assert_eq!((r.completed, r.errors), (20, 0));
}
