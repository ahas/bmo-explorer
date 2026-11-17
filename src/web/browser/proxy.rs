//! The one road out for a browser the web layer starts (docs/web-access.md, 「The browser's network path」). The
//! browser is told to send everything through this proxy, so it never resolves a name and connects on its own: the
//! proxy asks the caller's check for the address a name may be reached at and connects to exactly that
//! address, and a name the check refuses is never connected to at all.
//!
//! An https request passes as a tunnel, so TLS and the certificate check stay between the browser and the site.
//! A plain http request is sent on a connection of its own, and the connection ends with its answer, so one
//! connection never carries a second request to another name.
//!
//! Nothing here has a timeout: a connection is waited for until it opens or fails, and a request until the browser
//! has sent it whole or gone away.

use std::convert::Infallible;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderValue, Method, Request, Response, StatusCode, Uri};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

/// The address to connect to for a URL, none to connect by the name itself (a trusted road that checks
/// nothing), or a refusal.
pub type Resolver =
  Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<Option<IpAddr>, String>> + Send>> + Send + Sync>;

/// The resolver a browser's guard gives the proxy: the address the guard passes, or its refusal's words.
pub fn resolver_of(guard: &super::UrlGuard) -> Resolver {
  let guard = guard.clone();

  Arc::new(move |url: String| {
    let guard = guard.clone();

    Box::pin(async move { guard(url).await.map_err(|error| error.message) })
  })
}

/// A proxy standing for one browser; it stops when dropped.
pub struct PinnedProxy {
  pub port: u16,
  serving: JoinHandle<()>,
}

impl PinnedProxy {
  pub fn stop(&self) {
    self.serving.abort();
  }
}

impl Drop for PinnedProxy {
  fn drop(&mut self) {
    self.serving.abort();
  }
}

type Body = BoxBody<Bytes, hyper::Error>;

fn refusal(status: StatusCode) -> Response<Body> {
  let mut answer = Response::new(Empty::<Bytes>::new().map_err(|never| match never {}).boxed());

  *answer.status_mut() = status;
  answer.headers_mut().insert(http::header::CONNECTION, HeaderValue::from_static("close"));
  answer
}

// The connection to a host and port, once the check has answered.
async fn open(resolve: &Resolver, url: String, host: &str, port: u16) -> Result<TcpStream, String> {
  let address = resolve(url).await?;
  let connecting = match address {
    Some(address) => TcpStream::connect((address, port)).await,
    None => TcpStream::connect((host, port)).await,
  };

  connecting.map_err(|why| why.to_string())
}

fn bare(host: &str) -> &str {
  host.trim_start_matches('[').trim_end_matches(']')
}

async fn serve(request: Request<Incoming>, resolve: Resolver) -> Result<Response<Body>, Infallible> {
  if request.method() == Method::CONNECT {
    let Some(authority) = request.uri().authority().cloned() else {
      return Ok(refusal(StatusCode::BAD_REQUEST));
    };
    let host = bare(authority.host()).to_owned();
    let port = authority.port_u16().unwrap_or(443);
    let Ok(upstream) = open(&resolve, format!("https://{authority}/"), &host, port).await else {
      return Ok(refusal(StatusCode::FORBIDDEN));
    };

    tokio::spawn(async move {
      if let Ok(upgraded) = hyper::upgrade::on(request).await {
        let mut client = TokioIo::new(upgraded);
        let mut upstream = upstream;
        let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
      }
    });

    return Ok(Response::new(Empty::<Bytes>::new().map_err(|never| match never {}).boxed()));
  }

  let uri = request.uri().clone();

  if uri.scheme_str() != Some("http") {
    return Ok(refusal(StatusCode::BAD_REQUEST));
  }

  let Some(authority) = uri.authority().cloned() else {
    return Ok(refusal(StatusCode::BAD_REQUEST));
  };
  let host = bare(authority.host()).to_owned();
  let port = authority.port_u16().unwrap_or(80);
  let Ok(upstream) = open(&resolve, uri.to_string(), &host, port).await else {
    return Ok(refusal(StatusCode::FORBIDDEN));
  };
  let Ok((mut sender, connection)) = hyper::client::conn::http1::handshake(TokioIo::new(upstream)).await
  else {
    return Ok(refusal(StatusCode::BAD_GATEWAY));
  };

  tokio::spawn(connection);

  // The request as the origin server should see it: the path alone, no proxy headers, and the connection
  // closed after the answer.
  let (mut parts, body) = request.into_parts();
  let path = uri.path_and_query().map_or("/", |path| path.as_str()).to_owned();

  parts.uri = Uri::try_from(path).unwrap_or_else(|_| Uri::from_static("/"));
  for name in ["proxy-connection", "proxy-authorization", "connection", "keep-alive"] {
    parts.headers.remove(name);
  }
  parts.headers.insert(http::header::CONNECTION, HeaderValue::from_static("close"));

  match sender.send_request(Request::from_parts(parts, body)).await {
    Ok(answer) => Ok(answer.map(BodyExt::boxed)),
    Err(_) => Ok(refusal(StatusCode::BAD_GATEWAY)),
  }
}

/// A proxy on a free loopback port, every connection it makes checked by `resolve`.
pub async fn start(resolve: Resolver) -> std::io::Result<PinnedProxy> {
  let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
  let port = listener.local_addr()?.port();
  let serving = tokio::spawn(async move {
    // Every connection is held here, so stopping the proxy ends every one of them with it.
    let mut connections = JoinSet::new();

    loop {
      let Ok((client, _)) = listener.accept().await else {
        continue;
      };
      let resolve = resolve.clone();

      connections.spawn(async move {
        let service = service_fn(move |request| serve(request, resolve.clone()));
        let mut builder = hyper::server::conn::http1::Builder::new();

        // hyper's own default would end a connection whose request head is slow to come.
        builder.header_read_timeout(None);

        let _ = builder.serve_connection(TokioIo::new(client), service).with_upgrades().await;
      });

      while connections.try_join_next().is_some() {}
    }
  });

  Ok(PinnedProxy { port, serving })
}

/// What Chrome is told so that this proxy is its only road: every scheme through it, loopback included, and
/// no WebRTC UDP that would go around it.
pub fn chrome_arguments(port: u16) -> Vec<String> {
  vec![
    format!("--proxy-server=http://127.0.0.1:{port}"),
    "--proxy-bypass-list=<-loopback>".into(),
    "--force-webrtc-ip-handling-policy=disable_non_proxied_udp".into(),
  ]
}
