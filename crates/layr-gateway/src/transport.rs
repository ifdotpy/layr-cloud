//! Bounded HTTP/1.1 and HTTP/2 transport. Authenticate the edge before reading bodies.
use crate::server::{Gateway, Reply, Transfer, OBJECT_LIMIT};
use anyhow::{Context, Result};
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Body as _;
use hyper::{
    body::{Bytes, Incoming},
    service::service_fn,
    Request, Response,
};
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto::Builder,
};
use std::io::{self, Read};
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use std::{
    collections::BTreeMap,
    convert::Infallible,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Semaphore;

const CONNECTION_LIMIT: usize = 128;

type ResponseBody = http_body_util::combinators::UnsyncBoxBody<Bytes, io::Error>;
struct DownloadBody(tokio::sync::mpsc::Receiver<Result<hyper::body::Frame<Bytes>, io::Error>>);
impl hyper::body::Body for DownloadBody {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<Option<Result<hyper::body::Frame<Bytes>, io::Error>>> {
        self.0.poll_recv(cx)
    }
}
struct LeaseReader {
    reader: Box<dyn Read + Send>,
    _lease: tokio::sync::OwnedSemaphorePermit,
}
impl Read for LeaseReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.reader.read(bytes)
    }
}

fn response(mut reply: Reply, head: bool) -> Response<ResponseBody> {
    let mut response = Response::builder().status(reply.status);
    for (name, value) in reply.headers {
        response = response.header(name, value);
    }
    if let Some(Transfer::Download(mut object)) = reply.transfer.take() {
        response = response.header("content-length", object.length);
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        if !head {
            tokio::task::spawn_blocking(move || {
                let mut remaining = object.length;
                loop {
                    // A bounded channel and one 64 KiB block propagate client backpressure to S3.
                    let mut block = vec![0u8; (remaining.min(65536) as usize).max(1)];
                    let result = object.reader.read(&mut block);
                    match result {
                        Ok(0) if remaining == 0 => break,
                        Ok(0) => {
                            let _ = sender.blocking_send(Err(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "truncated release object",
                            )));
                            break;
                        }
                        Ok(n) if n as u64 <= remaining => {
                            remaining -= n as u64;
                            block.truncate(n);
                            if sender
                                .blocking_send(Ok(hyper::body::Frame::data(Bytes::from(block))))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Ok(_) => {
                            let _ = sender.blocking_send(Err(io::Error::other(
                                "release object exceeds length",
                            )));
                            break;
                        }
                        Err(error) => {
                            let _ = sender.blocking_send(Err(error));
                            break;
                        }
                    }
                }
            });
        }
        return response
            .body(DownloadBody(receiver).boxed_unsync())
            .unwrap();
    }
    response
        .header("content-length", reply.body.len())
        .body(
            Full::new(if head {
                Bytes::new()
            } else {
                Bytes::from(reply.body)
            })
            .map_err(|never| match never {})
            .boxed_unsync(),
        )
        .unwrap()
}

/// Pull HTTP frames only when S3 needs another part. No whole-body collection.
struct UploadReader {
    body: Incoming,
    pending: Bytes,
    runtime: tokio::runtime::Handle,
}
impl Read for UploadReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        while self.pending.is_empty() {
            match self.runtime.block_on(tokio::time::timeout(
                Duration::from_secs(120),
                self.body.frame(),
            )) {
                Ok(Some(Ok(frame))) => {
                    if let Ok(bytes) = frame.into_data() {
                        self.pending = bytes;
                    }
                }
                Ok(Some(Err(_))) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "upload disconnected",
                    ))
                }
                Ok(None) => return Ok(0),
                Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "upload stalled")),
            }
        }
        let n = output.len().min(self.pending.len());
        output[..n].copy_from_slice(&self.pending.split_to(n));
        Ok(n)
    }
}

fn release_path(target: &str) -> bool {
    let path = target.split('?').next().unwrap_or("");
    let key = path
        .split_once("/objects/")
        .map(|(_, key)| key)
        .or_else(|| {
            path.strip_prefix("/p/")
                .and_then(|p| p.split_once('/').map(|(_, key)| key))
        });
    key.and_then(crate::server::decode).is_some_and(|key| {
        let logical = key
            .strip_prefix("project/epochs/")
            .and_then(|suffix| suffix.split_once('/'))
            .filter(|(epoch, _)| layr_store::format::is_uuid(epoch))
            .map(|(_, key)| key)
            .unwrap_or(&key);
        layr_store::format::release_file(logical)
    })
}

// Bound both storage work and waiting requests across all connections/routes.
struct Admission {
    active: Arc<Semaphore>,
    waiting: Arc<Semaphore>,
    refused: std::sync::atomic::AtomicU64,
}
impl Admission {
    fn new() -> Self {
        Self { active: Arc::new(Semaphore::new(4)), waiting: Arc::new(Semaphore::new(64)), refused: std::sync::atomic::AtomicU64::new(0) }
    }
    fn refuse(&self, reason: &str) -> Reply {
        let count = self.refused.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        // Do not log paths or credentials: refusals happen before authorization.
        eprintln!("layr-gateway: admission refused reason={reason} refused_total={count}");
        let mut reply = Reply::error(503, "gateway is busy; retry later");
        reply.headers.push(("Retry-After".into(), "1".into()));
        reply
    }
    async fn acquire(&self, timeout: Duration) -> Result<tokio::sync::OwnedSemaphorePermit, Reply> {
        if let Ok(slot) = self.active.clone().try_acquire_owned() { return Ok(slot); }
        let _waiting = match self.waiting.clone().try_acquire_owned() {
            Ok(slot) => slot,
            Err(_) => return Err(self.refuse("queue_full")),
        };
        match tokio::time::timeout(timeout, self.active.clone().acquire_owned()).await {
            Ok(Ok(slot)) => Ok(slot),
            _ => Err(self.refuse("wait_timeout")),
        }
    }
}

async fn request(
    request: Request<Incoming>,
    gateway: Arc<Gateway>,
    peer: IpAddr,
    work: Arc<Admission>,
) -> Result<Reply> {
    let _slot = match work.acquire(Duration::from_secs(10)).await {
        Ok(slot) => slot,
        Err(reply) => return Ok(reply),
    };
    let (parts, body) = request.into_parts();
    let target = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();
    if target.len() > 4096
        || parts.headers.len() > 64
        || parts
            .headers
            .iter()
            .map(|(k, v)| k.as_str().len() + v.len() + 4)
            .sum::<usize>()
            > 16 * 1024
    {
        return Ok(Reply::error(431, "request headers are too large"));
    }
    let mut headers = BTreeMap::new();
    for (name, value) in &parts.headers {
        let value = match value.to_str() {
            Ok(value) => value,
            Err(_) => return Ok(Reply::error(400, "invalid header value")),
        };
        if headers
            .insert(name.to_string(), value.to_string())
            .is_some()
        {
            return Ok(Reply::error(400, "duplicate header"));
        }
    }
    if let Some(authority) = parts.uri.authority() {
        if headers
            .get("host")
            .is_some_and(|h| !h.eq_ignore_ascii_case(authority.as_str()))
        {
            return Ok(Reply::error(400, "conflicting host"));
        }
        headers.insert("host".into(), authority.to_string());
    }
    if let Err(reply) = gateway.client_address(&headers, peer) {
        return Ok(reply);
    }
    let method = parts.method.to_string();
    if matches!(method.as_str(), "GET" | "HEAD") && headers.contains_key("transfer-encoding") {
        return Ok(Reply::error(
            400,
            "GET and HEAD cannot use Transfer-Encoding",
        ));
    }
    if release_path(&target) && matches!(method.as_str(), "GET" | "PUT") {
        if method == "GET"
            && (!body.is_end_stream() || headers.get("content-length").is_some_and(|n| n != "0"))
        {
            return Ok(Reply::error(400, "GET has no body"));
        }
        let route_gateway = gateway.clone();
        let (mut reply, lease) = tokio::task::spawn_blocking(move || {
            (
                route_gateway.handle_from(
                    layr_store::http::Request {
                        method,
                        target,
                        headers,
                        body: Vec::new(),
                    },
                    None,
                    peer,
                ),
                _slot,
            )
        })
        .await?;
        match reply.transfer.take() {
            Some(Transfer::Upload {
                project,
                key,
                length,
            }) => {
                let runtime = tokio::runtime::Handle::current();
                return Ok(tokio::task::spawn_blocking(move || {
                    let _lease = lease;
                    let mut input = UploadReader {
                        body,
                        pending: Bytes::new(),
                        runtime,
                    };
                    match gateway
                        .storage
                        .put_stream(&project, &key, &mut input, length)
                    {
                        Ok(true) => Reply::bytes(201, Vec::new(), "application/json"),
                        Ok(false) => Reply::error(412, "object already exists"),
                        Err(_) => {
                            Reply::error(400, "release upload failed or exceeded its size limit")
                        }
                    }
                })
                .await?);
            }
            Some(Transfer::Download(mut object)) => {
                object.reader = Box::new(LeaseReader {
                    reader: object.reader,
                    _lease: lease,
                });
                reply.transfer = Some(Transfer::Download(object));
            }
            None => {}
        }
        return Ok(reply);
    }
    let data = match tokio::time::timeout(
        Duration::from_secs(120),
        Limited::new(body, OBJECT_LIMIT).collect(),
    )
    .await
    {
        Ok(Ok(data)) => data.to_bytes().to_vec(),
        Ok(Err(_)) => return Ok(Reply::error(413, "request body is too large or invalid")),
        Err(_) => return Ok(Reply::error(408, "request body timed out")),
    };
    if headers
        .get("content-length")
        .is_some_and(|v| v.parse::<usize>().ok() != Some(data.len()))
    {
        return Ok(Reply::error(400, "Content-Length does not match body"));
    }
    // Storage uses blocking HTTPS; keep it outside the async connection executor.
    Ok(tokio::task::spawn_blocking(move || {
        let _slot = _slot;
        gateway.handle_from(
            layr_store::http::Request {
                method,
                target,
                headers,
                body: data,
            },
            None,
            peer,
        )
    })
    .await?)
}

async fn connection<I>(io: I, gateway: Arc<Gateway>, peer: IpAddr, work: Arc<Admission>)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req: Request<Incoming>| {
        let gateway = gateway.clone();
        let work = work.clone();
        async move {
            let head = req.method() == hyper::Method::HEAD;
            let reply = request(req, gateway, peer, work)
                .await
                .unwrap_or_else(|_| Reply::error(502, "request failed"));
            Ok::<_, Infallible>(response(reply, head))
        }
    });
    let mut builder = Builder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(Duration::from_secs(15))
        .max_buf_size(32 * 1024)
        .keep_alive(false);
    builder
        .http2()
        .timer(TokioTimer::new())
        .max_concurrent_streams(4)
        .max_header_list_size(16 * 1024)
        .initial_stream_window_size(64 * 1024)
        .initial_connection_window_size(256 * 1024)
        .max_send_buf_size(64 * 1024);
    let _ = tokio::time::timeout(
        Duration::from_secs(7200),
        builder.serve_connection(TokioIo::new(io), service),
    )
    .await;
}

pub fn serve(
    address: SocketAddr,
    gateway: Arc<Gateway>,
    tls: Option<Arc<rustls::ServerConfig>>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let socket = socket2::Socket::new(
            if address.is_ipv6() {
                socket2::Domain::IPV6
            } else {
                socket2::Domain::IPV4
            },
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )?;
        if address.is_ipv6() {
            socket.set_only_v6(true)?;
        }
        socket.set_reuse_address(true)?;
        socket.set_nonblocking(true)?;
        socket.bind(&address.into())?;
        socket.listen(128)?;
        let listener = tokio::net::TcpListener::from_std(socket.into())
            .context("bind gateway listen address")?;
        let connections = Arc::new(Semaphore::new(CONNECTION_LIMIT));
        let work = Arc::new(Admission::new());
        eprintln!("layr-gateway: listening on {address}");
        loop {
            let (stream, peer) = listener.accept().await?;
            let Ok(slot) = connections.clone().try_acquire_owned() else {
                continue;
            };
            let gateway = gateway.clone();
            let work = work.clone();
            let tls = tls.clone();
            tokio::spawn(async move {
                let _slot = slot;
                if let Some(tls) = tls {
                    let acceptor = tokio_rustls::TlsAcceptor::from(tls);
                    if let Ok(Ok(stream)) =
                        tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await
                    {
                        connection(stream, gateway, peer.ip(), work).await;
                    }
                } else {
                    connection(stream, gateway, peer.ip(), work).await;
                }
            });
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn eight_request_burst_waits_without_exceeding_four_active_jobs() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let work = Arc::new(Admission::new());
            let active = Arc::new(AtomicUsize::new(0));
            let peak = Arc::new(AtomicUsize::new(0));
            // Occupy all workers first so every client must use the wait path.
            let occupied = work.active.clone().acquire_many_owned(4).await.unwrap();
            let mut jobs = Vec::new();
            for _ in 0..8 {
                let work = work.clone(); let active = active.clone(); let peak = peak.clone();
                jobs.push(tokio::spawn(async move {
                    let _lease = work.acquire(Duration::from_secs(1)).await.ok().expect("burst must drain");
                    let n = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(n, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                }));
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert_eq!(active.load(Ordering::SeqCst), 0);
            drop(occupied);
            for job in jobs { job.await.unwrap(); }
            assert_eq!(peak.load(Ordering::SeqCst), 4);
            assert_eq!(work.active.available_permits(), 4);
            let occupied = work.active.clone().acquire_many_owned(4).await.unwrap();
            let refusal = work.acquire(Duration::from_millis(5)).await.err().expect("sustained overload must remain bounded");
            assert_eq!(refusal.status, 503);
            assert!(refusal.headers.contains(&("Retry-After".into(), "1".into())));
            drop(occupied);
            assert_eq!(work.active.available_permits(), 4);
        });
    }
    #[test]
    fn real_transport_queues_eight_and_sixty_four_and_refuses_full_queue() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for count in [8, 64] {
                let fixture = crate::server::tests::Fixture::new();
                let gateway = fixture.transport_gateway();
                let work = Arc::new(Admission::new());
                let occupied = work.active.clone().acquire_many_owned(4).await.unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let accept_work = work.clone();
                let server = tokio::spawn(async move {
                    let mut peers = Vec::new();
                    for _ in 0..count + 1 {
                        let (io, peer) = listener.accept().await.unwrap();
                        peers.push(tokio::spawn(connection(io, gateway.clone(), peer.ip(), accept_work.clone())));
                    }
                    for peer in peers { peer.await.unwrap(); }
                });
                fn client(address: SocketAddr) -> String {
                    use std::io::Write;
                    let mut stream = std::net::TcpStream::connect(address).unwrap();
                    stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
                    stream.write_all(b"GET /no-such-route HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
                    let mut response = String::new(); stream.read_to_string(&mut response).unwrap(); response
                }
                let mut clients = Vec::new();
                for _ in 0..count { clients.push(tokio::task::spawn_blocking(move || client(address))); }
                tokio::time::timeout(Duration::from_secs(5), async {
                    while work.waiting.available_permits() != 64 - count { tokio::task::yield_now().await; }
                }).await.expect("all real HTTP requests must enter the queue");
                if count == 64 {
                    let refused = tokio::task::spawn_blocking(move || client(address)).await.unwrap();
                    assert!(refused.starts_with("HTTP/1.1 503"));
                    assert!(refused.to_ascii_lowercase().contains("retry-after: 1"));
                    assert_eq!(work.refused.load(std::sync::atomic::Ordering::Relaxed), 1);
                } else {
                    clients.push(tokio::task::spawn_blocking(move || client(address)));
                }
                drop(occupied);
                for response in clients { assert!(response.await.unwrap().starts_with("HTTP/1.1 404")); }
                server.await.unwrap();
                assert_eq!(work.waiting.available_permits(), 64);
                assert_eq!(work.active.available_permits(), 4);
            }
        });
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn tls_http1_full_queue_with_128_connections_has_bounded_rss() {
        // Other tests intentionally allocate large record stores. Measure admission
        // in a fresh test process so their retained allocator pages do not count.
        if std::env::var_os("LAYR_GATEWAY_RSS_CHILD").is_none() {
            let result=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","transport::tests::tls_http1_full_queue_with_128_connections_has_bounded_rss","--nocapture","--test-threads=1"])
                .env("LAYR_GATEWAY_RSS_CHILD","1").output().unwrap();
            print!("{}",String::from_utf8_lossy(&result.stdout));
            eprint!("{}",String::from_utf8_lossy(&result.stderr));
            assert!(result.status.success());return;
        }
        use std::io::{BufRead, Write};
        fn rss() -> u64 {
            std::fs::read_to_string("/proc/self/status").unwrap().lines().find_map(|line|
                line.strip_prefix("VmRSS:").map(|value|value.split_whitespace().next().unwrap().parse::<u64>().unwrap()*1024)).unwrap()
        }
        let root=std::path::PathBuf::from("/dev/shm").join(format!("layr-transport-{}",uuid::Uuid::now_v7()));
        std::fs::create_dir(&root).unwrap();
        let certificate=root.join("certificate.pem");let key=root.join("key.pem");
        assert!(std::process::Command::new("openssl").args(["req","-x509","-newkey","rsa:2048","-nodes","-days","1","-subj","/CN=localhost","-addext","subjectAltName=DNS:localhost","-keyout"]).arg(&key).arg("-out").arg(&certificate).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap().success());
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let baseline=rss();let fixture=crate::server::tests::Fixture::new();let gateway=fixture.transport_gateway();
            let provider=crate::tls::provider();
            let resolver=crate::tls::Reloading::new(certificate.clone(),key.clone(),provider.clone()).unwrap();
            let mut tls=rustls::ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_cert_resolver(Arc::new(resolver));
            tls.alpn_protocols=vec![b"http/1.1".to_vec()];let acceptor=tokio_rustls::TlsAcceptor::from(Arc::new(tls));
            let work=Arc::new(Admission::new());let occupied=work.active.clone().acquire_many_owned(4).await.unwrap();
            let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();
            let connections=Arc::new(Semaphore::new(CONNECTION_LIMIT));let server_connections=connections.clone();let server_work=work.clone();
            let server=tokio::spawn(async move {
                let mut jobs=Vec::new();
                for _ in 0..129 {
                    let (io,peer)=listener.accept().await.unwrap();
                    let permit=server_connections.clone().try_acquire_owned().expect("connection cap must admit 128");
                    let acceptor=acceptor.clone();let gateway=gateway.clone();let work=server_work.clone();
                    jobs.push(tokio::spawn(async move {
                        let _permit=permit;
                        let stream=tokio::time::timeout(Duration::from_secs(10),acceptor.accept(io)).await.unwrap().unwrap();
                        connection(stream,gateway,peer.ip(),work).await;
                    }));
                }
                for job in jobs { job.await.unwrap(); }
            });
            let script=r#"import socket,ssl,sys
ctx=ssl.create_default_context(cafile=sys.argv[1]);ctx.set_alpn_protocols(['http/1.1'])
port=int(sys.argv[2]);held=[];idle=[]
def connect():
 s=ctx.wrap_socket(socket.create_connection(('127.0.0.1',port),timeout=15),server_hostname='localhost');s.settimeout(15);return s
def request(s):s.sendall(b'GET /no-such-route HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n')
def read(s):
 data=b''
 while True:
  block=s.recv(65536)
  if not block:break
  data+=block
 return data
for i in range(64):
 s=connect();request(s);held.append(s)
print('queued',flush=True);assert sys.stdin.readline().strip()=='continue'
s=connect();request(s);response=read(s);s.close();assert response.startswith(b'HTTP/1.1 503') and b'retry-after: 1' in response.lower()
for i in range(64):idle.append(connect())
print('open128',flush=True);assert sys.stdin.readline().strip()=='release'
for s in held:assert read(s).startswith(b'HTTP/1.1 404');s.close()
for s in idle:s.close()
print('passed',flush=True)
"#;
            let mut child=std::process::Command::new("python3").arg("-c").arg(script).arg(&certificate).arg(address.port().to_string()).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
            let mut output=std::io::BufReader::new(child.stdout.take().unwrap());let mut input=child.stdin.take().unwrap();
            let mut line=String::new();output.read_line(&mut line).unwrap();assert_eq!(line.trim(),"queued");
            tokio::time::timeout(Duration::from_secs(5),async {while work.waiting.available_permits()!=0 {tokio::task::yield_now().await;}}).await.unwrap();
            writeln!(input,"continue").unwrap();line.clear();output.read_line(&mut line).unwrap();assert_eq!(line.trim(),"open128");
            tokio::time::timeout(Duration::from_secs(5),async {while connections.available_permits()!=0 {tokio::task::yield_now().await;}}).await.unwrap();
            let resident=rss();println!("{{\"connections\":128,\"waiters\":64,\"activeLimit\":4,\"http\":\"1.1 over TLS\",\"baselineRssBytes\":{baseline},\"loadedRssBytes\":{resident}}}");
            assert!(resident<128*1024*1024,"transport RSS must leave ample nano memory");
            assert_eq!(work.refused.load(std::sync::atomic::Ordering::Relaxed),1);
            drop(occupied);writeln!(input,"release").unwrap();line.clear();output.read_line(&mut line).unwrap();assert_eq!(line.trim(),"passed");
            assert!(child.wait().unwrap().success());server.await.unwrap();
        });
        std::fs::remove_dir_all(root).unwrap();
    }

}
