//! The connection under HTTP: TCP, then TLS for `https`.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use async_io::Timer;
use async_net::TcpStream;
use futures::{AsyncRead, AsyncWrite};
use futures_rustls::client::TlsStream;
use futures_rustls::pki_types::ServerName;
use futures_rustls::rustls::ClientConfig;

use crate::NetError;
use crate::target::Target;

/// A connection, plain (loopback `http` only) or over TLS, whose reads a
/// [`ReadLimit`] may cap.
pub(crate) struct Stream {
    transport: Transport,
    limit: Option<ReadLimit>,
}

enum Transport {
    Plain(Watched),
    Tls(Box<TlsStream<Watched>>),
}

/// The most each read of a [`Stream`] may take, shared with whoever reads the
/// response's body. Above TLS, so it counts what HTTP sees: TLS itself reads a
/// record at a time beneath it.
///
/// hyper reads the socket only once its buffer is empty and it has handed over
/// its last frame (a client's body channel holds one). So a reader that takes
/// each frame and lowers this to what it may still accept is never handed more
/// than that: the framing and head a read also takes only use up the read.
#[derive(Clone, Debug)]
pub(crate) struct ReadLimit(Arc<LimitState>);

#[derive(Debug)]
struct LimitState {
    remaining: AtomicU64,
    /// Every byte the stream has read, for the tests to measure.
    read: AtomicU64,
}

impl ReadLimit {
    pub(crate) fn new(bytes: u64) -> ReadLimit {
        ReadLimit(Arc::new(LimitState {
            remaining: AtomicU64::new(bytes),
            read: AtomicU64::new(0),
        }))
    }

    /// Caps the next reads at `bytes`.
    pub(crate) fn set(&self, bytes: u64) {
        self.0.remaining.store(bytes, Ordering::Relaxed);
    }

    /// The most a read into `buf` may fill: never nothing, which would read
    /// as the end of the stream.
    pub(crate) fn cap(&self, buf: usize) -> usize {
        let remaining = self.0.remaining.load(Ordering::Relaxed);
        usize::try_from(remaining).map_or(buf, |remaining| remaining.max(1).min(buf))
    }

    #[cfg(test)]
    pub(crate) fn total_read(&self) -> u64 {
        self.0.read.load(Ordering::Relaxed)
    }
}

impl Stream {
    /// Resolves and connects to `target`, then shakes hands with `tls` when
    /// the target is `https`. A read that waits `idle` for the socket's next
    /// bytes fails with [`io::ErrorKind::TimedOut`].
    pub(crate) async fn connect(
        target: &Target,
        idle: Duration,
        tls: impl FnOnce() -> Result<Arc<ClientConfig>, NetError>,
    ) -> Result<Stream, NetError> {
        let tcp = TcpStream::connect((target.host.as_str(), target.port))
            .await
            .map_err(|error| {
                NetError::Connect(format!("{}:{}: {error}", target.host, target.port))
            })?;
        // The request goes out in one write; don't hold it back.
        let _ = tcp.set_nodelay(true);
        let tcp = Watched {
            tcp,
            idle,
            timer: None,
        };
        if !target.tls {
            return Ok(Stream::new(Transport::Plain(tcp)));
        }
        let name = ServerName::try_from(target.host.clone())
            .map_err(|error| NetError::Tls(format!("{}: {error}", target.host)))?;
        let tls = futures_rustls::TlsConnector::from(tls()?)
            .connect(name, tcp)
            .await
            .map_err(|error| NetError::Tls(format!("{}: {error}", target.host)))?;
        Ok(Stream::new(Transport::Tls(Box::new(tls))))
    }

    fn new(transport: Transport) -> Stream {
        Stream {
            transport,
            limit: None,
        }
    }

    /// Caps this stream's reads by `limit`, if there's one.
    pub(crate) fn limited(mut self, limit: Option<ReadLimit>) -> Stream {
        self.limit = limit;
        self
    }
}

/// A TCP stream whose reads time out: a read fails once it has waited the
/// idle timeout without a byte, counted from when it began waiting, so
/// anything the server sends (a header line, chunk framing, a TLS record's
/// first bytes) keeps it going.
pub(crate) struct Watched {
    tcp: TcpStream,
    idle: Duration,
    /// Set while a read waits.
    timer: Option<Timer>,
}

impl AsyncRead for Watched {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        match Pin::new(&mut this.tcp).poll_read(cx, buf) {
            Poll::Ready(result) => {
                this.timer = None;
                Poll::Ready(result)
            }
            Poll::Pending => {
                let idle = this.idle;
                let timer = this.timer.get_or_insert_with(|| Timer::after(idle));
                match Pin::new(timer).poll(cx) {
                    Poll::Ready(_) => {
                        this.timer = None;
                        Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            format!("nothing was received for {idle:?}"),
                        )))
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

impl AsyncWrite for Watched {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().tcp).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().tcp).poll_write_vectored(cx, bufs)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().tcp).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().tcp).poll_close(cx)
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let Some(limit) = &this.limit else {
            return this.transport.poll_read(cx, buf);
        };
        let cap = limit.cap(buf.len());
        let read = this.transport.poll_read(cx, &mut buf[..cap]);
        if let Poll::Ready(Ok(bytes)) = read {
            limit.0.read.fetch_add(bytes as u64, Ordering::Relaxed);
        }
        read
    }
}

impl Transport {
    fn poll_read(&mut self, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
        match self {
            Transport::Plain(tcp) => Pin::new(tcp).poll_read(cx, buf),
            Transport::Tls(tls) => Pin::new(tls).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().transport {
            Transport::Plain(tcp) => Pin::new(tcp).poll_write(cx, buf),
            Transport::Tls(tls) => Pin::new(tls).poll_write(cx, buf),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().transport {
            Transport::Plain(tcp) => Pin::new(tcp).poll_write_vectored(cx, bufs),
            Transport::Tls(tls) => Pin::new(tls).poll_write_vectored(cx, bufs),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().transport {
            Transport::Plain(tcp) => Pin::new(tcp).poll_flush(cx),
            Transport::Tls(tls) => Pin::new(tls).poll_flush(cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().transport {
            Transport::Plain(tcp) => Pin::new(tcp).poll_close(cx),
            Transport::Tls(tls) => Pin::new(tls).poll_close(cx),
        }
    }
}
