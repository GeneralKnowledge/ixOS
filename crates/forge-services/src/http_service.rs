//! `native_services!(http)`: the UniFFI glue between `forge_net::HttpClient`
//! and the host's HTTP stack. The logic is in [`crate::http`].

#[cfg(feature = "http")]
#[doc(hidden)]
#[macro_export]
macro_rules! __native_http {
    () => {
        /// Ordinary HTTP requests through the host's networking stack (iOS:
        /// `URLSession`). See `forge_services::http` and
        /// docs/native-services.md.
        pub mod http {
            use ::std::sync::Arc;

            pub use $crate::http::{HostClient, PreferInstalled};

            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct HttpHeader {
                pub name: String,
                pub value: String,
            }

            /// One request for the host to send. `id` names it for
            /// `HttpTransport::cancel`.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct HttpRequest {
                pub id: u64,
                /// `GET`, `POST`, `PUT`, `PATCH` or `DELETE`.
                pub method: String,
                pub url: String,
                pub headers: Vec<HttpHeader>,
                pub body: Vec<u8>,
                /// Send this file as the body instead (an upload); `body` is
                /// then empty.
                pub body_file: Option<String>,
                pub timeout_ms: u64,
                /// Stop and fail with `Transport` once the response body
                /// grows past this.
                pub max_body_bytes: u64,
            }

            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct HttpResponse {
                pub status: u16,
                pub headers: Vec<HttpHeader>,
                pub body: Vec<u8>,
            }

            /// Why a request got no response. A non-2xx status is a
            /// response, not an error.
            #[derive(::uniffi::Error, Clone, Debug, PartialEq, Eq)]
            pub enum HttpTransportError {
                /// No route, DNS failure, connection refused, no network.
                Offline { reason: String },
                /// The server's certificate was rejected.
                Certificate { reason: String },
                Timeout,
                Cancelled,
                InvalidRequest { reason: String },
                /// Anything else (TLS protocol errors, a body too large).
                Transport { reason: String },
            }

            impl ::std::fmt::Display for HttpTransportError {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    ::std::fmt::Display::fmt(&to_net_error(self.clone()), f)
                }
            }

            impl ::std::error::Error for HttpTransportError {}

            impl ::std::convert::From<::uniffi::UnexpectedUniFFICallbackError> for HttpTransportError {
                fn from(err: ::uniffi::UnexpectedUniFFICallbackError) -> Self {
                    Self::Transport { reason: err.reason }
                }
            }

            fn to_net_error(err: HttpTransportError) -> $crate::http::net::NetError {
                use $crate::http::net::NetError;
                match err {
                    HttpTransportError::Offline { reason } => NetError::Offline { reason },
                    HttpTransportError::Certificate { reason } => NetError::Certificate { reason },
                    HttpTransportError::Timeout => NetError::Timeout,
                    HttpTransportError::Cancelled => NetError::Cancelled,
                    HttpTransportError::InvalidRequest { reason } => {
                        NetError::InvalidRequest { reason }
                    }
                    HttpTransportError::Transport { reason } => NetError::Transport { reason },
                }
            }

            /// The status and headers of a streamed response.
            #[derive(::uniffi::Record, Clone, Debug, PartialEq, Eq)]
            pub struct HttpStreamHead {
                pub status: u16,
                pub headers: Vec<HttpHeader>,
            }

            /// Where the host delivers a streamed body, in order. None of
            /// its calls block.
            #[derive(::uniffi::Object)]
            pub struct HttpStreamBody {
                feed: $crate::http::StreamFeed,
            }

            #[::uniffi::export]
            impl HttpStreamBody {
                /// The next bytes of the body.
                pub fn receive(&self, data: Vec<u8>) {
                    self.feed.push(data);
                }

                /// The server closed the body normally.
                pub fn finish(&self) {
                    self.feed.finish();
                }

                /// The body stopped with an error (`Cancelled` when Rust
                /// cancelled it).
                pub fn fail(&self, error: HttpTransportError) {
                    self.feed.fail(to_net_error(error));
                }
            }

            /// Implemented by the host (iOS: `URLSession`). `send` and
            /// `open_stream` block their calling thread, a Rust worker
            /// thread.
            #[::uniffi::export(with_foreign)]
            pub trait HttpTransport: Send + Sync {
                fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpTransportError>;
                /// Returns once the status and headers have arrived (within
                /// `timeout_ms`), then delivers the body into `body` until
                /// it ends, fails or is cancelled. The body has no size
                /// limit and no idle timeout.
                fn open_stream(
                    &self,
                    request: HttpRequest,
                    body: Arc<HttpStreamBody>,
                ) -> Result<HttpStreamHead, HttpTransportError>;
                fn cancel(&self, id: u64);
            }

            struct Host(Arc<dyn HttpTransport>);

            fn host_request(request: $crate::http::HostRequest) -> HttpRequest {
                let $crate::http::HostRequest {
                    id,
                    request,
                    body_file,
                    max_body_bytes,
                } = request;
                let timeout = request.timeout.unwrap_or($crate::http::DEFAULT_TIMEOUT);
                HttpRequest {
                    id,
                    method: request.method.as_str().to_string(),
                    url: request.url,
                    headers: request
                        .headers
                        .into_iter()
                        .map(|h| HttpHeader {
                            name: h.name,
                            value: h.value,
                        })
                        .collect(),
                    body: if body_file.is_some() { Vec::new() } else { request.body },
                    body_file: body_file.map(|p| p.to_string_lossy().into_owned()),
                    timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
                    max_body_bytes,
                }
            }

            fn net_headers(headers: Vec<HttpHeader>) -> Vec<$crate::http::net::Header> {
                headers
                    .into_iter()
                    .map(|h| $crate::http::net::Header {
                        name: h.name,
                        value: h.value,
                    })
                    .collect()
            }

            impl $crate::http::HostHttp for Host {
                fn send(
                    &self,
                    request: $crate::http::HostRequest,
                ) -> Result<$crate::http::net::Response, $crate::http::net::NetError> {
                    let response = self.0.send(host_request(request)).map_err(to_net_error)?;
                    Ok($crate::http::net::Response {
                        status: response.status,
                        headers: net_headers(response.headers),
                        body: response.body,
                    })
                }

                fn open_stream(
                    &self,
                    request: $crate::http::HostRequest,
                    feed: $crate::http::StreamFeed,
                ) -> Result<$crate::http::StreamHead, $crate::http::net::NetError> {
                    let head = self
                        .0
                        .open_stream(host_request(request), Arc::new(HttpStreamBody { feed }))
                        .map_err(to_net_error)?;
                    Ok($crate::http::StreamHead {
                        status: head.status,
                        headers: net_headers(head.headers),
                    })
                }

                fn cancel(&self, id: u64) {
                    self.0.cancel(id);
                }
            }

            /// Called by the host at launch (`ForgeServices.install()`).
            #[::uniffi::export]
            pub fn install_native_http(transport: Arc<dyn HttpTransport>) {
                HttpService::new(transport).install();
            }

            /// The host's HTTP stack for Rust code. Cheap to clone.
            #[derive(Clone)]
            pub struct HttpService {
                client: HostClient,
            }

            $crate::__native_slot!(HttpService, "http");

            impl HttpService {
                pub fn new(transport: Arc<dyn HttpTransport>) -> Self {
                    Self {
                        client: HostClient::new(Arc::new(Host(transport))),
                    }
                }

                /// A `forge_net::HttpClient` sending through the host.
                pub fn client(&self) -> HostClient {
                    self.client.clone()
                }

                /// A client that sends through the host once it has
                /// installed this service, and through `fallback` before
                /// that and where it never does (Linux tests, Android).
                /// Make the app's client with this: the core is often
                /// created before the host installs its services.
                /// `configure` sets limits on the host client (timeout,
                /// body size) to match the fallback's.
                pub fn prefer_installed(
                    fallback: Arc<dyn $crate::http::net::HttpClient>,
                    configure: impl Fn(HostClient) -> HostClient + Send + Sync + 'static,
                ) -> PreferInstalled {
                    PreferInstalled::new(
                        move || {
                            HttpService::installed().ok().map(|service| {
                                Arc::new(configure(service.client()))
                                    as Arc<dyn $crate::http::net::HttpClient>
                            })
                        },
                        fallback,
                    )
                }
            }
        }
    };
}

#[cfg(not(feature = "http"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __native_http {
    () => {
        compile_error!(
            "native_services!(http) needs forge-services' `http` feature: \
             forge-services = { ..., features = [\"http\"] }"
        );
    };
}
