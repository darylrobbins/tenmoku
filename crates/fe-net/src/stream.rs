//! The connection under HTTP: TCP, then TLS for `https`.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
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

/// A connection, plain (loopback `http` only) or over TLS.
pub(crate) enum Stream {
    Plain(Watched),
    Tls(Box<TlsStream<Watched>>),
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
            return Ok(Stream::Plain(tcp));
        }
        let name = ServerName::try_from(target.host.clone())
            .map_err(|error| NetError::Tls(format!("{}: {error}", target.host)))?;
        let tls = futures_rustls::TlsConnector::from(tls()?)
            .connect(name, tcp)
            .await
            .map_err(|error| NetError::Tls(format!("{}: {error}", target.host)))?;
        Ok(Stream::Tls(Box::new(tls)))
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
        match self.get_mut() {
            Stream::Plain(tcp) => Pin::new(tcp).poll_read(cx, buf),
            Stream::Tls(tls) => Pin::new(tls).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Plain(tcp) => Pin::new(tcp).poll_write(cx, buf),
            Stream::Tls(tls) => Pin::new(tls).poll_write(cx, buf),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Plain(tcp) => Pin::new(tcp).poll_write_vectored(cx, bufs),
            Stream::Tls(tls) => Pin::new(tls).poll_write_vectored(cx, bufs),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(tcp) => Pin::new(tcp).poll_flush(cx),
            Stream::Tls(tls) => Pin::new(tls).poll_flush(cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(tcp) => Pin::new(tcp).poll_close(cx),
            Stream::Tls(tls) => Pin::new(tls).poll_close(cx),
        }
    }
}
