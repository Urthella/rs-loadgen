use std::fmt;

/// Latency percentiles in microseconds, by nearest rank.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub count: usize,
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub p999: u64,
    pub max: u64,
    pub mean: f64,
}

impl Summary {
    // ponytail: keeps every sample and sorts them, exact but 8 bytes a request; an HDR histogram
    // when runs reach hundreds of millions of requests.
    pub fn of(mut micros: Vec<u64>) -> Summary {
        if micros.is_empty() {
            return Summary::default();
        }
        micros.sort_unstable();
        let rank =
            |p: f64| micros[((p * micros.len() as f64).ceil() as usize).clamp(1, micros.len()) - 1];
        Summary {
            count: micros.len(),
            p50: rank(0.50),
            p90: rank(0.90),
            p99: rank(0.99),
            p999: rank(0.999),
            max: *micros.last().unwrap(),
            mean: micros.iter().sum::<u64>() as f64 / micros.len() as f64,
        }
    }
}

fn ms(micros: u64) -> String {
    format!("{:.1} ms", micros as f64 / 1000.0)
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "p50 {}  p90 {}  p99 {}  p99.9 {}  max {}",
            ms(self.p50),
            ms(self.p90),
            ms(self.p99),
            ms(self.p999),
            ms(self.max)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank() {
        let s = Summary::of((1..=1000).rev().collect());
        assert_eq!(
            (s.count, s.p50, s.p90, s.p99, s.p999, s.max),
            (1000, 500, 900, 990, 999, 1000)
        );
        assert_eq!(s.mean, 500.5);
        let one = Summary::of(vec![7]);
        assert_eq!((one.p50, one.p999, one.max), (7, 7, 7));
        assert_eq!(Summary::of(vec![]), Summary::default());
        // With 10 samples, p99 is the largest: rank ceil(9.9) = 10.
        assert_eq!(Summary::of((1..=10).collect()).p99, 10);
    }
}
