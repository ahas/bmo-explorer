//! bmo-node (docs/api.md, 「bmo-node」): the node list a remote browser's node is found in, and the connection a
//! remote node is reached by, through its hub.

use reqwest::Method;
use serde::Deserialize;

use super::{Unit, segment};

/// One node of the list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Node {
  pub name: String,
  #[serde(default)]
  pub label: String,
  /// `ssh` or `remote`.
  pub kind: String,
}

/// A remote node's connection: its hub's address, the file the hub's token stands in, and the agent the node is
/// attached as.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
  pub kind: String,
  #[serde(default)]
  pub hub_url: Option<String>,
  #[serde(default)]
  pub token_file: Option<String>,
  #[serde(default)]
  pub agent: Option<String>,
}

#[derive(Deserialize)]
struct Listed {
  #[serde(default)]
  nodes: Vec<Node>,
}

/// bmo-node at the address the settings name, under the token they name.
#[derive(Clone, Debug)]
pub struct NodeClient {
  unit: Unit,
}

impl NodeClient {
  pub fn new(address: Option<String>, token: Option<String>) -> Self {
    Self {
      unit: Unit::new("bmo-node", "node", address, token),
    }
  }

  /// One node of the list by its name, none where the list holds no such node.
  pub async fn node(&self, name: &str) -> Result<Option<Node>, String> {
    let listed: Listed = self.unit.ask(Method::GET, "/nodes", &[], None).await?;

    Ok(listed.nodes.into_iter().find(|node| node.name == name))
  }

  /// A node's connection for a tool that reaches it by name, none where the list holds no such node.
  pub async fn target(&self, name: &str) -> Result<Option<Target>, String> {
    self.unit.found(&format!("/nodes/{}/target", segment(name)), &[]).await
  }
}
