use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::net::TcpStream;

use bmo_explorer::web::chrome::proxy::*;

// An origin on loopback answering every request with what it was asked.
async fn origin() -> u16 {
  let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
  let port = listener.local_addr().unwrap().port();

  tokio::spawn(async move {
    loop {
      let (mut socket, _) = listener.accept().await.unwrap();

      tokio::spawn(async move {
        let mut asked = vec![0; 4_096];
        let read = socket.read(&mut asked).await.unwrap_or(0);
        let head = String::from_utf8_lossy(&asked[..read]).into_owned();
        let line = head.lines().next().unwrap_or("").to_owned();
        let answer =
          format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{line}", line.len());

        let _ = socket.write_all(answer.as_bytes()).await;
      });
    }
  });

  port
}

fn trusting() -> Resolver {
  Arc::new(|_| Box::pin(async { Ok(Some("127.0.0.1".parse().unwrap())) }))
}

fn refusing() -> Resolver {
  Arc::new(|_| Box::pin(async { Err("refused".to_owned()) }))
}

async fn ask(proxy: u16, head: &str) -> String {
  let mut socket = TcpStream::connect(("127.0.0.1", proxy)).await.unwrap();

  socket.write_all(head.as_bytes()).await.unwrap();

  let mut answer = Vec::new();
  let _ = tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut answer)).await;

  String::from_utf8_lossy(&answer).into_owned()
}

// Carried from packages/web/pinned-proxy.test.ts: sends a plain request as the origin sees it, on a connection that ends with the answer
#[tokio::test]
async fn a_plain_request_reaches_the_checked_address_with_its_path_alone() {
  let origin = origin().await;
  let proxy = start(trusting()).await.unwrap();
  let answer = ask(proxy.port, &format!("GET http://site.example:{origin}/a?b=1 HTTP/1.1\r\nhost: site.example\r\nproxy-connection: keep-alive\r\n\r\n")).await;

  assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
  assert!(answer.ends_with("GET /a?b=1 HTTP/1.1"), "{answer}");
}

// Carried from packages/web/pinned-proxy.test.ts: refuses what the check refuses, before connecting anywhere
#[tokio::test]
async fn a_name_the_check_refuses_is_never_connected_to() {
  let origin = origin().await;
  let proxy = start(refusing()).await.unwrap();

  assert!(
    ask(
      proxy.port,
      &format!("GET http://site.example:{origin}/ HTTP/1.1\r\nhost: site.example\r\n\r\n")
    )
    .await
    .starts_with("HTTP/1.1 403")
  );
  assert!(
    ask(
      proxy.port,
      &format!("CONNECT site.example:{origin} HTTP/1.1\r\nhost: site.example\r\n\r\n")
    )
    .await
    .starts_with("HTTP/1.1 403")
  );
}

// Carried from packages/web/pinned-proxy.test.ts: tunnels to the address the check answered, not to the name
#[tokio::test]
async fn a_tunnel_carries_bytes_both_ways_to_the_checked_address() {
  let origin = origin().await;
  let proxy = start(trusting()).await.unwrap();
  let mut socket = TcpStream::connect(("127.0.0.1", proxy.port)).await.unwrap();

  socket
    .write_all(
      format!("CONNECT site.example:{origin} HTTP/1.1\r\nhost: site.example:{origin}\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap();

  let mut established = vec![0; 1_024];
  let read = socket.read(&mut established).await.unwrap();

  assert!(String::from_utf8_lossy(&established[..read]).starts_with("HTTP/1.1 200"));

  socket.write_all(b"GET /tunnelled HTTP/1.1\r\n\r\n").await.unwrap();

  let mut answer = Vec::new();
  let _ = tokio::time::timeout(Duration::from_secs(5), socket.read_to_end(&mut answer)).await;

  assert!(String::from_utf8_lossy(&answer).ends_with("GET /tunnelled HTTP/1.1"));
}

// Carried from packages/web/pinned-proxy.test.ts: answers a request it cannot read with a refusal
#[tokio::test]
async fn a_request_it_cannot_read_is_answered_with_a_refusal() {
  let proxy = start(trusting()).await.unwrap();

  assert!(
    ask(proxy.port, "GET /relative HTTP/1.1\r\nhost: x\r\n\r\n")
      .await
      .starts_with("HTTP/1.1 400")
  );
  assert!(ask(proxy.port, "NOT A REQUEST\r\n\r\n").await.starts_with("HTTP/1.1 400"));
}

// Carried from packages/web/pinned-proxy.test.ts: tells Chrome to send everything through it, loopback included
#[test]
fn chrome_is_told_the_proxy_is_its_only_road() {
  assert_eq!(
    chrome_arguments(9),
    [
      "--proxy-server=http://[::1]:9",
      "--proxy-bypass-list=<-loopback>",
      "--force-webrtc-ip-handling-policy=disable_non_proxied_udp"
    ]
  );
}
