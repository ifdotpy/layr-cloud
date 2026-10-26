//! Bounded HTTP/1.1 and HTTP/2 transport. The socket peer supplies rate-limit identity.
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

async fn request(
    request: Request<Incoming>,
    gateway: Arc<Gateway>,
    peer: IpAddr,
    work: Arc<Semaphore>,
) -> Result<Reply> {
    let _slot = match work.try_acquire_owned() {
        Ok(slot) => slot,
        Err(_) => return Ok(Reply::error(503, "gateway is busy; retry later")),
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

async fn connection<I>(io: I, gateway: Arc<Gateway>, peer: IpAddr, work: Arc<Semaphore>)
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
        let connections = Arc::new(Semaphore::new(32));
        let work = Arc::new(Semaphore::new(4));
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
