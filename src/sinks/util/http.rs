use super::retries::RetryLogic;
use futures::{Async, Future, Poll};
use hyper::{
    client::{HttpConnector, ResponseFuture},
    Body, Client,
};
use hyper_tls::HttpsConnector;
use std::sync::Arc;
use std::time::Instant;
use tokio::executor::DefaultExecutor;
use tokio_trace::{field, Span};
use tower::Service;

type RequestBuilder = Box<dyn Fn(Vec<u8>) -> hyper::Request<Body> + Sync + Send>;

#[derive(Clone)]
pub struct HttpService {
    client: Client<HttpsConnector<HttpConnector>, Body>,
    request_builder: Arc<RequestBuilder>,
}

impl HttpService {
    pub fn new(
        request_builder: impl Fn(Vec<u8>) -> hyper::Request<Body> + Sync + Send + 'static,
    ) -> Self {
        let https = HttpsConnector::new(4).expect("TLS initialization failed");
        let client: Client<_, Body> = Client::builder()
            .executor(DefaultExecutor::current())
            .build(https);
        Self {
            client,
            request_builder: Arc::new(Box::new(request_builder)),
        }
    }
}

impl Service<Vec<u8>> for HttpService {
    type Response = hyper::Response<Body>;
    type Error = hyper::Error;
    type Future = Instrumented;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(().into())
    }

    fn call(&mut self, body: Vec<u8>) -> Self::Future {
        let request = (self.request_builder)(body);

        let start = Instant::now();
        let span = info_span!(
            "request",
            method = &field::debug(request.method()),
            version = &field::debug(request.version()),
            uri = &field::debug(request.uri()),
        );
        trace!(
            message = "sending.",
            headers = &field::debug(request.headers())
        );
        let inner = self.client.request(request.into());

        Instrumented { inner, span, start }
    }
}

pub struct Instrumented {
    inner: ResponseFuture,
    span: Span,
    start: Instant,
}

impl Future for Instrumented {
    type Item = <ResponseFuture as Future>::Item;
    type Error = <ResponseFuture as Future>::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let span = self.span.clone();
        let start = self.start;

        span.enter(|| match self.inner.poll() {
            Ok(Async::Ready(res)) => {
                let end = Instant::now();
                let duration = end.duration_since(start);
                debug!(
                    message = "response.",
                    status = &field::display(res.status()),
                    version = &field::debug(res.version()),
                    duration_ms = &field::debug(duration.as_millis())
                );
                Ok(Async::Ready(res))
            }

            p => p,
        })
    }
}

#[derive(Clone)]
pub struct HttpRetryLogic;

impl RetryLogic for HttpRetryLogic {
    type Error = hyper::Error;
    type Response = hyper::Response<Body>;

    fn is_retriable_error(&self, error: &Self::Error) -> bool {
        error.is_connect() || error.is_closed()
    }

    fn should_retry_response(&self, response: &Self::Response) -> bool {
        response.status().is_server_error()
    }
}

#[cfg(test)]
mod test {
    use super::HttpService;
    use futures::{Future, Sink, Stream};
    use http::Method;
    use hyper::service::service_fn;
    use hyper::{Body, Response, Server, Uri};
    use tower::Service;

    #[test]
    fn it_makes_http_requests() {
        let addr = crate::test_util::next_addr();
        let uri = format!("http://{}:{}/", addr.ip(), addr.port())
            .parse::<Uri>()
            .unwrap();

        let request = b"hello".to_vec();
        let mut service = HttpService::new(move |body| {
            let mut builder = hyper::Request::builder();
            builder.method(Method::POST);
            builder.uri(uri.clone());
            builder.body(body.into()).unwrap()
        });

        let req = service.call(request);

        let (tx, rx) = futures::sync::mpsc::channel(10);

        let new_service = move || {
            let tx = tx.clone();

            service_fn(move |req: hyper::Request<Body>| -> Box<dyn Future<Item = Response<Body>, Error = String> + Send> {
                let tx = tx.clone();

                Box::new(req.into_body().map_err(|_| "".to_string()).fold::<_, _, Result<_, String>>(vec![], |mut acc, chunk| {
                    acc.extend_from_slice(&chunk);
                    Ok(acc)
                }).and_then(move |v| {
                    let string = String::from_utf8(v).map_err(|_| "Wasn't UTF-8".to_string());
                    tx.send(string).map_err(|_| "Send error".to_string())
                }).and_then(|_| {
                    futures::future::ok(Response::new(Body::from("")))
                }))
            })
        };

        let server = Server::bind(&addr)
            .serve(new_service)
            .map_err(|e| eprintln!("server error: {}", e));

        let mut rt = tokio::runtime::Runtime::new().unwrap();

        rt.spawn(server);

        rt.block_on(req).unwrap();

        rt.shutdown_now();

        let (body, _rest) = rx.into_future().wait().unwrap();
        assert_eq!(body.unwrap().unwrap(), "hello");
    }
}
