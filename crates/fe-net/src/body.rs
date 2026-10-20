//! A response body that streams as it arrives, driving its connection as
//! it is read: no executor runs the connection, so any executor runs the
//! client.

use http_body_util::Full;
use hyper::body::{Body as _, Bytes, Incoming};
use hyper::client::conn::http1::Connection;
use smol_hyper::rt::FuturesIo;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::NetError;
use crate::stream::Stream;

/// hyper's half of a connection: polled, it reads and writes the socket.
pub(crate) type Conn = Connection<FuturesIo<Stream>, Full<Bytes>>;

/// A connection, polled until it ends. Dropping it closes the socket.
pub(crate) struct Driver {
    conn: Option<Conn>,
    /// Why the connection failed, if it did: hyper tells the response or
    /// body only that the connection broke, and this says how.
    error: Option<hyper::Error>,
}

impl Driver {
    pub(crate) fn new(conn: Conn) -> Driver {
        Driver {
            conn: Some(conn),
            error: None,
        }
    }

    /// Reads and writes what the connection can. A failure isn't returned
    /// here: the response or body it breaks reports it, through
    /// [`Driver::error`].
    pub(crate) fn drive(&mut self, cx: &mut Context<'_>) {
        if let Some(conn) = &mut self.conn
            && let Poll::Ready(result) = Pin::new(conn).poll(cx)
        {
            self.conn = None;
            self.error = result.err();
        }
    }

    /// The error that broke the response or body: the connection's own
    /// when it failed, else `error`.
    pub(crate) fn error(&mut self, error: hyper::Error) -> NetError {
        NetError::from_hyper(self.error.take().unwrap_or(error))
    }
}

/// A response body: its chunks as they arrive, chunked or
/// `content-length` alike. Dropping it closes the connection.
pub struct Body {
    driver: Driver,
    /// `None` once the body ended or failed.
    incoming: Option<Incoming>,
}

impl Body {
    pub(crate) fn new(driver: Driver, incoming: Incoming) -> Body {
        Body {
            driver,
            incoming: Some(incoming),
        }
    }

    /// Ends the body and closes the connection.
    fn end(&mut self) {
        self.incoming = None;
        self.driver.conn = None;
    }
}

impl futures::Stream for Body {
    type Item = Result<Vec<u8>, NetError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            let Some(incoming) = &mut this.incoming else {
                return Poll::Ready(None);
            };
            this.driver.drive(cx);
            match Pin::new(incoming).poll_frame(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    // Trailers are dropped, as are empty chunks.
                    if let Ok(data) = frame.into_data()
                        && !data.is_empty()
                    {
                        return Poll::Ready(Some(Ok(data.to_vec())));
                    }
                }
                Poll::Ready(Some(Err(error))) => {
                    let error = this.driver.error(error);
                    this.end();
                    return Poll::Ready(Some(Err(error)));
                }
                Poll::Ready(None) => {
                    this.end();
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.incoming {
            Some(incoming) if incoming.is_end_stream() => (0, Some(0)),
            Some(_) => (0, None),
            None => (0, Some(0)),
        }
    }
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Body")
            .field("ended", &self.incoming.is_none())
            .finish_non_exhaustive()
    }
}
