//! Test helper: a local HTTP server that plays back fixed replies.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;

/// Reads one whole HTTP request, headers and body. The client may send them in separate
/// writes; replying and closing after only the first one resets the connection under it.
fn read_request(stream: &mut impl Read) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let text = String::from_utf8_lossy(&data);
        if let Some(head_end) = text.find("\r\n\r\n") {
            let length = text[..head_end]
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if data.len() >= head_end + 4 + length {
                return text.into_owned();
            }
        }
        let n = stream.read(&mut buf).unwrap();
        assert!(n > 0, "connection closed before the request was complete");
        data.extend_from_slice(&buf[..n]);
    }
}

/// Serves one reply per connection, in order: (status, extra header lines each ending in
/// "\r\n", body). Returns the server's base URL and a handle that yields the requests it
/// received, headers and body, once every reply has been served.
pub fn mock_server(replies: Vec<(u16, &'static str, &'static str)>) -> (String, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, headers, body) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            requests.push(read_request(&mut stream));
            let reply = format!(
                "HTTP/1.1 {status} Reply\r\nContent-Type: application/json\r\nConnection: close\r\n{headers}Content-Length: {}\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
        requests
    });
    (base, server)
}
