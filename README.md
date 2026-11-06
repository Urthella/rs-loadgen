# rs-loadgen

[![ci](https://github.com/Urthella/rs-loadgen/actions/workflows/ci.yml/badge.svg)](https://github.com/Urthella/rs-loadgen/actions/workflows/ci.yml)

An HTTP load generator that sends requests at a fixed rate and measures each one's latency from
when it was supposed to be sent. Plain Rust, no dependencies.

That one choice decides whether a load test can see a stall. Here's a server that answers in 1 ms,
except for one second when it answers nothing, tested for 5 seconds at 500 requests a second:

| How latency is measured | Requests answered | p50 | p90 | p99 | p99.9 | max |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| From when each was really sent, skipping the ones a slow response delayed | 2,049 of 2,500 | 1.7 ms | **1.9 ms** | 961 ms | 997 ms | 1,001 ms |
| From when each was due | 2,500 of 2,500 | 2.1 ms | **511 ms** | 954 ms | 998 ms | 1,004 ms |

```
cargo run --release --example stall
```

A fifth of the requests were due during the stall, and each one waited for it to end, so their
true latencies run from 1 s down to almost nothing. That puts the true p90 at about 500 ms, which
is what the second row measures. The first row reports 1.9 ms: while the server was stalled it
stopped sending, so the requests that would have shown the stall were never made. Gil Tene calls
this coordinated omission, and it's how a load generator measures by default when it waits for each
response before sending the next request.

## Use it

```bash
cargo run --release -- http://localhost:8080/health --rate 500 --duration 30 --connections 50
```

```
15000 requests to localhost:8080/health at 500/s over 50 connections, open model
p50 3.8 ms  p90 4.2 ms  p99 5.4 ms  p99.9 16.8 ms  max 16.8 ms
15000 of 15000 scheduled requests answered, 0 errors, 30.0 s
```

`--timeline` adds latency second by second, which shows when a run went wrong, not just how
much. Here the server stalled for a second that straddled the first two:

```
second   0:    200 requests  p50 575.7 ms  p90 913.4 ms  p99 992.4 ms  p99.9 1002.1 ms  max 1002.1 ms
second   1:    200 requests  p50 5.8 ms  p90 299.4 ms  p99 374.3 ms  p99.9 384.0 ms  max 384.0 ms
second   2:    200 requests  p50 5.5 ms  p90 5.8 ms  p99 6.1 ms  p99.9 6.4 ms  max 6.4 ms
second   3:    200 requests  p50 5.5 ms  p90 5.8 ms  p99 7.3 ms  p99.9 7.8 ms  max 7.8 ms
```

Requests are counted in the second they were due, so each second has its 200 even when the
server answered none of them on time. With `--closed`, they're counted in the second they were
sent, and the stalled second is nearly empty.

`--closed` measures the other way, for comparison. Use enough connections that `rate × latency`
fits: a request due while every connection is busy waits for one, and that wait counts.

## How it works

Request `k` is due at `start + k / rate`. In the open model, whichever connection is free takes
the next request, waits until it's due if it's early, sends it, and records `now - due`. When the
server stalls, every connection gets stuck, the requests due meanwhile queue up, and each one's
wait is counted when it finally goes out.

In the closed model, connection `w` takes requests `w`, `w + n`, `w + 2n`… of the same schedule,
records `now - sent`, and after a slow response skips the slots that went by. It sends fewer
requests exactly when the server is slow.

This is what [wrk2](https://github.com/giltene/wrk2) fixed in wrk; [oha](https://github.com/hatoo/oha)
has `--latency-correction`, and k6 has arrival-rate executors. This repo is a small, readable
version with an experiment that shows the difference.

## How it's tested

The tests run against a built-in server that stalls for a known second, so the true latencies are
known. In the open model, all 600 requests are answered and p90 lands between 0.5 and 0.9 s, where
the theory puts it at 0.7 s for that run. In the closed model, fewer than 500 are sent and p90 stays
under 100 ms. The timeline puts the slow requests in the stalled second in the open model, and
almost no requests at all there in the closed one. Without a stall, both agree. Other tests cover HTTP parsing (lengths, chunks,
`Connection: close`, HTTP/1.0, 204), reconnecting to a server that closes after every response, an
unreachable server, and the command line.

To check that the tests can fail, 12 bugs were planted one at a time, and every one was caught:
among them, the open model measuring from when it sent, the closed model catching up instead of
skipping, and reusing a connection the server closed.

## Limits

- **Plain HTTP/1.1 GET only.** No TLS, no other methods, no request bodies.
- **A thread per connection**, which is fine for hundreds of connections, not tens of thousands.
- **Every latency is kept and sorted**: exact, but 8 bytes a request. A 10-minute run at 10,000
  requests a second takes about 48 MB.
- **The generator's own pauses count too.** If the machine running it stalls, that shows up as
  latency, which is the honest answer but not the server's fault.

Claude, Anthropic's AI model, wrote this with Halil Utku Demirtaş directing the work.

## License

MIT
