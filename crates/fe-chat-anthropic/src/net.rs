//! [`Transport`] over [`fe_net::Client`]: the HTTPS one.

use futures::StreamExt;
use futures::future::BoxFuture;
use http::header::{HeaderName, HeaderValue};
use http::{HeaderMap, Method};

use crate::transport::{Response, Transport, TransportError};

impl Transport for fe_net::Client {
    fn post(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    ) -> BoxFuture<'static, Result<Response, TransportError>> {
        let sent =
            header_map(headers).map(|headers| self.request(Method::POST, url, headers, body));
        Box::pin(async move {
            let response = sent?.await.map_err(transport_error)?;
            Ok(Response {
                status: response.status.as_u16(),
                headers: response.headers,
                body: response
                    .body
                    .map(|chunk| chunk.map_err(transport_error))
                    .boxed(),
            })
        })
    }
}

fn header_map(headers: Vec<(String, String)>) -> Result<HeaderMap, TransportError> {
    let mut map = HeaderMap::with_capacity(headers.len());
    for (name, value) in headers {
        let name = HeaderName::try_from(name.as_str())
            .map_err(|error| TransportError::new(format!("header {name:?}: {error}")))?;
        // The value may be the API key: it isn't put in the error.
        let value = HeaderValue::try_from(value)
            .map_err(|error| TransportError::new(format!("header {name}'s value: {error}")))?;
        map.append(name, value);
    }
    Ok(map)
}

fn transport_error(error: fe_net::NetError) -> TransportError {
    TransportError::new(error.to_string())
}
