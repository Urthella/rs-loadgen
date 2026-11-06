use std::process::ExitCode;
use std::time::Duration;

use rs_loadgen::http::Target;
use rs_loadgen::{Mode, Plan, run};

const USAGE: &str = "usage: rs-loadgen URL [--rate 100] [--duration 10] [--connections 20] [--timeout 10] [--closed] [--timeline]

  --rate N          requests per second, across all connections
  --duration S      seconds to run
  --connections N   connections to keep open; more than rate × latency, or requests queue
  --timeout S       give up on a request after this many seconds
  --closed          measure like most load generators do, for comparison: from when each
                    request was really sent, skipping the ones a slow response delayed
  --timeline        also show latency second by second";

/// The plan, and whether to show the timeline.
fn parse(args: &[String]) -> Result<(Plan, bool), String> {
    let mut plan = Plan {
        target: Target::parse(args.first().ok_or("no URL given")?)?,
        rate: 100.0,
        duration: Duration::from_secs(10),
        connections: 20,
        mode: Mode::Open,
        timeout: Duration::from_secs(10),
    };
    let mut timeline = false;
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        let mut number = || -> Result<f64, String> {
            let value = rest.next().ok_or(format!("{flag} needs a value"))?;
            value
                .parse::<f64>()
                .ok()
                .filter(|v| *v > 0.0)
                .ok_or(format!("{flag} needs a positive number, not {value}"))
        };
        match flag.as_str() {
            "--rate" => plan.rate = number()?,
            "--duration" => plan.duration = Duration::from_secs_f64(number()?),
            "--connections" => plan.connections = number()? as usize,
            "--timeout" => plan.timeout = Duration::from_secs_f64(number()?),
            "--closed" => plan.mode = Mode::Closed,
            "--timeline" => timeline = true,
            _ => return Err(format!("unknown option: {flag}")),
        }
    }
    if plan.connections == 0 {
        return Err("--connections needs at least 1".into());
    }
    Ok((plan, timeline))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "-h" || args[0] == "--help" {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let (plan, timeline) = match parse(&args) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mode = if plan.mode == Mode::Open {
        "open"
    } else {
        "closed"
    };
    println!(
        "{} requests to {}:{}{} at {}/s over {} connections, {mode} model",
        (plan.rate * plan.duration.as_secs_f64()).round(),
        plan.target.host,
        plan.target.port,
        plan.target.path,
        plan.rate,
        plan.connections
    );
    let report = run(&plan);
    if timeline {
        for (second, summary) in report.timeline.iter().enumerate() {
            println!(
                "second {second:>3}: {:>6} requests  {summary}",
                summary.count
            );
        }
    }
    println!("{}", report.latency);
    println!(
        "{} of {} scheduled requests answered, {} errors, {:.1} s",
        report.completed,
        report.scheduled,
        report.errors,
        report.elapsed.as_secs_f64()
    );
    if report.errors > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn options() {
        let (plan, timeline) = parse(&args(
            "http://localhost:8080/ --rate 250 --duration 2.5 --connections 8 --timeout 3 --closed --timeline",
        ))
        .unwrap();
        assert_eq!(
            (plan.rate, plan.connections, plan.mode, timeline),
            (250.0, 8, Mode::Closed, true)
        );
        assert_eq!(
            (plan.duration, plan.timeout),
            (Duration::from_millis(2500), Duration::from_secs(3))
        );
        let (defaults, timeline) = parse(&args("http://localhost/")).unwrap();
        assert_eq!(
            (defaults.rate, defaults.connections, defaults.mode, timeline),
            (100.0, 20, Mode::Open, false)
        );
    }

    #[test]
    fn bad_options() {
        for line in [
            "",
            "https://x/",
            "http://x/ --rate",
            "http://x/ --rate 0",
            "http://x/ --rate abc",
            "http://x/ --connections 0.5",
            "http://x/ --fast",
        ] {
            assert!(parse(&args(line)).is_err(), "{line:?}");
        }
    }
}
