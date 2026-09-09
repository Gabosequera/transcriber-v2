use crossbeam_channel::{Receiver, Sender, bounded};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const MAX_BODY: usize = 1024 * 1024;
const MAX_HEADER: usize = 16 * 1024;

pub struct PendingRequest {
    pub message: Value,
    reply: Sender<Value>,
    deadline: Instant,
}
impl PendingRequest {
    /// Expired requests are removed before GUI dispatch, never applied later.
    pub fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }
    pub fn respond(self, response: Value) {
        let _ = self.reply.try_send(response);
    }
}

pub struct ControlService {
    address: SocketAddr,
    token: String,
    requests: Receiver<PendingRequest>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl ControlService {
    pub fn start() -> io::Result<Self> {
        Self::start_with_wake(|| {})
    }
    pub fn start_with_wake(wake: impl Fn() + Send + Sync + 'static) -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        // 288 random bits. Never written to logs, project or discovery files.
        let token: String = (0..6).map(|_| tv2_domain::ids::random_hex12()).collect();
        let (sender, requests) = bounded(16);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_token = token.clone();
        let wake = Arc::new(wake);
        let worker = thread::Builder::new().name("tv2-mcp-listener".into()).spawn(move || {
            // Bounded worker pool: stalled clients cannot allocate unlimited threads.
            let (connections, incoming) = bounded::<TcpStream>(8);
            let mut workers = Vec::new();
            for _ in 0..4 {
                let incoming = incoming.clone();
                let sender = sender.clone();
                let token = worker_token.clone();
                let wake = wake.clone();
                workers.push(thread::spawn(move || {
                    for stream in incoming {
                        let _ = serve(stream, &token, address, &sender, &*wake);
                    }
                }));
            }
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, peer)) if peer.ip().is_loopback() => {
                        let _ = connections.try_send(stream);
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(20)),
                    Err(_) => break,
                }
            }
            drop(connections);
            // Request channels close when workers leave. Dropping the service never blocks GUI.
            drop(workers);
        })?;
        Ok(Self { address, token, requests, stop, worker: Some(worker) })
    }
    pub fn endpoint(&self) -> String {
        format!("http://{}/mcp", self.address)
    }
    pub fn token(&self) -> &str {
        &self.token
    }
    pub fn try_recv(&self) -> Option<PendingRequest> {
        loop {
            let request = self.requests.try_recv().ok()?;
            if !request.expired() {
                return Some(request);
            }
        }
    }
}
impl Drop for ControlService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take();
    }
}

fn response(stream: &mut TcpStream, status: &str, body: Option<&Value>) -> io::Result<()> {
    let bytes = body.map(serde_json::to_vec).transpose()?.unwrap_or_default();
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nAllow: POST, GET\r\n\r\n",
        bytes.len()
    )?;
    stream.write_all(&bytes)
}
fn constant_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}
fn serve(mut stream: TcpStream, token: &str, address: SocketAddr, requests: &Sender<PendingRequest>, wake: &dyn Fn()) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut raw = Vec::new();
    let mut headers = BTreeMap::new();
    let mut start = String::new();
    for index in 0..100 {
        let before = raw.len();
        let remaining = MAX_HEADER.saturating_sub(before);
        if remaining == 0 {
            return response(&mut stream, "431 Request Header Fields Too Large", None);
        }
        (&mut reader).take(remaining as u64).read_until(b'\n', &mut raw)?;
        let line = std::str::from_utf8(&raw[before..]).unwrap_or("").trim_end_matches(['\r', '\n']);
        if index == 0 {
            start = line.to_owned();
            continue;
        }
        if line.is_empty() {
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            return response(&mut stream, "400 Bad Request", None);
        };
        if headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_owned()).is_some() {
            return response(&mut stream, "400 Bad Request", None);
        }
    }
    if !raw.ends_with(b"\r\n\r\n") {
        return response(&mut stream, "400 Bad Request", None);
    }
    let host = address.to_string();
    if headers.get("host") != Some(&host) {
        return response(&mut stream, "403 Forbidden", None);
    }
    if headers.get("origin").is_some_and(|origin| origin != &format!("http://{host}")) {
        return response(&mut stream, "403 Forbidden", None);
    }
    if !headers.get("authorization").is_some_and(|value| constant_equal(value.as_bytes(), format!("Bearer {token}").as_bytes())) {
        return response(&mut stream, "401 Unauthorized", None);
    }
    if start == "GET /mcp HTTP/1.1" || start == "DELETE /mcp HTTP/1.1" {
        return response(&mut stream, "405 Method Not Allowed", None);
    }
    if start != "POST /mcp HTTP/1.1" {
        return response(&mut stream, "404 Not Found", None);
    }
    if headers.get("mcp-protocol-version").is_some_and(|version| version != crate::MCP_VERSION && version != "2025-03-26") {
        return response(&mut stream, "400 Bad Request", None);
    }
    if headers.contains_key("transfer-encoding") || !headers.get("content-type").is_some_and(|v| v.split(';').next() == Some("application/json")) {
        return response(&mut stream, "415 Unsupported Media Type", None);
    }
    let Some(length) = headers.get("content-length").and_then(|n| n.parse::<usize>().ok()) else {
        return response(&mut stream, "411 Length Required", None);
    };
    if length > MAX_BODY {
        return response(&mut stream, "413 Content Too Large", None);
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let message: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return response(
                &mut stream,
                "400 Bad Request",
                Some(&json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}})),
            );
        }
    };
    if !message.is_object() || message["jsonrpc"] != "2.0" {
        return response(&mut stream, "400 Bad Request", None);
    }
    if message.get("id").is_none() {
        if message["method"] == "notifications/initialized" || message["method"] == "notifications/cancelled" {
            return response(&mut stream, "202 Accepted", None);
        }
        return response(&mut stream, "400 Bad Request", None);
    }
    let (reply, receiver) = bounded(1);
    let pending = PendingRequest { message, reply, deadline: Instant::now() + Duration::from_secs(30) };
    if requests.try_send(pending).is_err() {
        return response(&mut stream, "503 Service Unavailable", None);
    }
    wake();
    match receiver.recv_timeout(Duration::from_secs(30)) {
        Ok(value) => response(&mut stream, "200 OK", Some(&value)),
        Err(_) => response(&mut stream, "504 Gateway Timeout", None),
    }
}
