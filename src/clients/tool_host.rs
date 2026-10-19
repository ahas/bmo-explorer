//! The tool host's control socket (docs/api.md, 「The tool host」): HTTP/1.1 over a Unix socket, at the path
//! `BMO_MCP_HOST_SOCKET` names, or `bmo/mcp-host.sock` under the user's runtime directory. What the `web` namespace
//! says of its site tools is read here, and which entry claims a page.

use std::path::{Path, PathBuf};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};

/// The variable naming the control socket where it does not stand at its default path.
pub const SOCKET_VARIABLE: &str = "BMO_MCP_HOST_SOCKET";

/// The tool host's control socket.
#[derive(Clone, Debug)]
pub struct ToolHost {
  socket: PathBuf,
}

impl ToolHost {
  /// The socket `BMO_MCP_HOST_SOCKET` names, or the default one under the runtime directory.
  pub fn from_environment() -> Self {
    let socket = std::env::var_os(SOCKET_VARIABLE).filter(|named| !named.is_empty()).map_or_else(
      || {
        std::env::var_os("XDG_RUNTIME_DIR")
          .map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
          .join("bmo")
          .join("mcp-host.sock")
      },
      PathBuf::from,
    );

    Self { socket }
  }

  pub fn at(socket: &Path) -> Self {
    Self {
      socket: socket.to_path_buf(),
    }
  }

  /// One question on the socket: its status and its answer read as JSON, null where the answer holds none.
  pub async fn ask(
    &self,
    method: Method,
    path: &str,
    body: Option<&Value>,
  ) -> Result<(u16, Value), String> {
    let unanswered =
      |why: String| format!("the tool host does not answer on {}: {why}", self.socket.display());
    let stream = tokio::net::UnixStream::connect(&self.socket)
      .await
      .map_err(|why| unanswered(why.to_string()))?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
      .await
      .map_err(|why| unanswered(why.to_string()))?;

    tokio::spawn(connection);

    let request = Request::builder()
      .method(method)
      .uri(path)
      .header("host", "localhost")
      .header("content-type", "application/json")
      .body(Full::new(Bytes::from(body.map(Value::to_string).unwrap_or_default())))
      .map_err(|why| unanswered(why.to_string()))?;
    let answer = sender.send_request(request).await.map_err(|why| unanswered(why.to_string()))?;
    let status = answer.status().as_u16();
    let bytes = answer
      .into_body()
      .collect()
      .await
      .map_err(|why| unanswered(why.to_string()))?
      .to_bytes();

    Ok((status, serde_json::from_slice(&bytes).unwrap_or(Value::Null)))
  }

  /// The site tools' catalogue, as the `web` namespace's facts carry it.
  pub async fn site_catalogue(&self) -> Result<Vec<Value>, String> {
    let (status, facts) = self.ask(Method::GET, "/facts/web", None).await?;

    match facts["sites"].as_array() {
      Some(sites) if status == 200 => Ok(sites.clone()),
      _ => Err("the tool host keeps no site catalogue".to_owned()),
    }
  }

  /// The site tool a record holding `web.sites` would have read an address with, by its name.
  pub async fn claimed_by(&self, address: &str, data_root: &Path) -> Result<Option<String>, String> {
    let record = json!({
      "line": "shell",
      "capabilities": ["web.sites"],
      "dataRoot": data_root,
      "runVariables": { "BMO_TURN_PRIVILEGED": "1" },
    });
    let (_, claimed) = self
      .ask(Method::POST, "/claims", Some(&json!({ "session": record, "address": address })))
      .await?;

    Ok(
      claimed["entry"]
        .as_str()
        .map(|entry| entry.strip_prefix("web/").unwrap_or(entry).to_owned()),
    )
  }
}
