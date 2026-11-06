//! A test server that answers every request after a fixed delay, and can stall: hold every
//! response for a while, as a garbage collection pause or a lock would.

use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct Behavior {
    /// How long each response takes.
    pub service: Duration,
    /// From this moment, for this long, responses wait until the stall is over.
    pub stall: Option<(Instant, Duration)>,
}

/// Starts the server on a free local port, in background threads that live as long as the process.
pub fn spawn(behavior: Behavior) -> io::Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::spawn(move || {
                let _ = serve(stream, behavior);
            });
        }
    });
    Ok(addr)
}

fn serve(stream: TcpStream, behavior: Behavior) -> io::Result<()> {
    stream.set_nodelay(true)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    loop {
        // Read one request: everything up to the empty line. GET has no body.
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                return Ok(());
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
        }
        if let Some((from, length)) = behavior.stall {
            let now = Instant::now();
            if now >= from && now < from + length {
                thread::sleep(from + length - now);
            }
        }
        thread::sleep(behavior.service);
        writer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")?;
    }
}
