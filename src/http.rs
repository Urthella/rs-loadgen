//! Just enough HTTP/1.1 for a load generator: GET over a kept-alive connection, with bodies by
//! Content-Length, chunked, or until the server closes.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Target {
    /// Parses `http://host[:port][/path]`. HTTPS isn't supported.
    pub fn parse(url: &str) -> Result<Target, String> {
        let rest = url
            .strip_prefix("http://")
            .ok_or("only http:// URLs are supported")?;
        let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, port.parse().map_err(|_| format!("bad port: {port}"))?),
            None => (authority, 80),
        };
        if host.is_empty() {
            return Err("no host".into());
        }
        let path = if path.is_empty() { "/" } else { path };
        Ok(Target {
            host: host.into(),
            port,
            path: path.into(),
        })
    }
}

pub struct Conn {
    reader: BufReader<TcpStream>,
    request: Vec<u8>,
    /// False once the server said it will close the connection.
    pub reusable: bool,
}

impl Conn {
    pub fn connect(target: &Target, timeout: Duration) -> io::Result<Conn> {
        let addr = (target.host.as_str(), target.port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "the host has no address"))?;
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_nodelay(true)?;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}:{}\r\nUser-Agent: rs-loadgen\r\n\r\n",
            target.path, target.host, target.port
        );
        Ok(Conn {
            reader: BufReader::new(stream),
            request: request.into_bytes(),
            reusable: true,
        })
    }

    /// Sends one request and reads the whole response. Returns the status code.
    pub fn get(&mut self) -> io::Result<u16> {
        self.reader.get_mut().write_all(&self.request)?;
        let (status, keep_alive) = read_response(&mut self.reader)?;
        self.reusable = keep_alive;
        Ok(status)
    }
}

fn line(reader: &mut impl BufRead) -> io::Result<String> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

/// Reads a response, discarding the body. Returns the status and whether the connection stays open.
pub fn read_response(reader: &mut impl BufRead) -> io::Result<(u16, bool)> {
    let status_line = line(reader)?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    let status: u16 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| bad("bad status line"))?;
    // HTTP/1.0 closes by default; HTTP/1.1 keeps the connection open.
    let mut keep_alive = version == "HTTP/1.1";
    let (mut length, mut chunked) = (None, false);
    loop {
        let header = line(reader)?;
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err(bad("bad header"));
        };
        let value = value.trim().to_ascii_lowercase();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                length = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| bad("bad content-length"))?,
                )
            }
            "transfer-encoding" => chunked = value.split(',').any(|v| v.trim() == "chunked"),
            "connection" => keep_alive = value != "close" && (keep_alive || value == "keep-alive"),
            _ => {}
        }
    }
    // 1xx, 204 and 304 have no body.
    if (100..200).contains(&status) || status == 204 || status == 304 {
        return Ok((status, keep_alive));
    }
    if chunked {
        loop {
            let size_line = line(reader)?;
            let size = u64::from_str_radix(size_line.split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| bad("bad chunk size"))?;
            if size == 0 {
                // Trailers, then the empty line that ends them.
                while !line(reader)?.is_empty() {}
                break;
            }
            io::copy(&mut reader.by_ref().take(size), &mut io::sink())?;
            line(reader)?;
        }
    } else if let Some(length) = length {
        let read = io::copy(&mut reader.by_ref().take(length), &mut io::sink())?;
        if read < length {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
    } else {
        // No length: the body runs until the server closes.
        io::copy(reader, &mut io::sink())?;
        keep_alive = false;
    }
    Ok((status, keep_alive))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(raw: &str) -> io::Result<(u16, bool)> {
        read_response(&mut raw.as_bytes())
    }

    #[test]
    fn urls() {
        assert_eq!(
            Target::parse("http://localhost:8080/a?b=1").unwrap(),
            Target {
                host: "localhost".into(),
                port: 8080,
                path: "/a?b=1".into()
            }
        );
        assert_eq!(
            Target::parse("http://example.com").unwrap(),
            Target {
                host: "example.com".into(),
                port: 80,
                path: "/".into()
            }
        );
        assert!(Target::parse("https://example.com").is_err());
        assert!(Target::parse("http://:80/").is_err());
        assert!(Target::parse("http://host:x/").is_err());
    }

    #[test]
    fn bodies_by_length_chunks_or_close() {
        assert_eq!(
            read("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").unwrap(),
            (200, true)
        );
        let chunked = "HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\n\r\n3;x=y\r\nabc\r\nA\r\n0123456789\r\n0\r\nTrailer: 1\r\n\r\n";
        assert_eq!(read(chunked).unwrap(), (201, true));
        assert_eq!(
            read("HTTP/1.1 200 OK\r\n\r\nuntil close").unwrap(),
            (200, false)
        );
        assert_eq!(
            read("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").unwrap(),
            (200, false)
        );
        assert_eq!(
            read("HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n").unwrap(),
            (200, false)
        );
        assert_eq!(
            read("HTTP/1.0 200 OK\r\nConnection: keep-alive\r\nContent-Length: 0\r\n\r\n").unwrap(),
            (200, true)
        );
        assert_eq!(
            read("HTTP/1.1 204 No Content\r\n\r\n").unwrap(),
            (204, true)
        );
    }

    #[test]
    fn two_responses_on_one_connection_are_read_apart() {
        let mut both = "HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabcHTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".as_bytes();
        assert_eq!(read_response(&mut both).unwrap(), (200, true));
        assert_eq!(read_response(&mut both).unwrap(), (404, true));
    }

    #[test]
    fn broken_responses_are_errors() {
        assert!(read("").is_err());
        assert!(read("HTTP/1.1 abc OK\r\n\r\n").is_err());
        assert!(read("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort").is_err());
        assert!(read("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err());
        assert!(read("HTTP/1.1 200 OK\r\nno colon\r\n\r\n").is_err());
    }
}
