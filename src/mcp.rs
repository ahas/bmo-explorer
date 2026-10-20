//! `bmo-explorer mcp`: the harness plugin's MCP server on standard input and output (docs/api.md, 「The MCP
//! server」). Each tool is a thin call of one client function; the server lists them and the OpenAPI document
//! states them under `x-tools`, both from the one table here.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rmcp::model::{
  CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, InitializeResult,
  JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use schemars::generate::{SchemaGenerator, SchemaSettings};
use schemars::{JsonSchema, Schema};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::client::{
  ClientError, Explorer, LinkPreview, NoInput, PageSource, PageText, Raster, RasterInput,
  SearchEnginesOutput, UrlInput, WebSearchInput, WebSearchOutput,
};

/// The plugin a harness takes the explorer in, and the command that starts its MCP server.
pub const PLUGIN: &str = "bmo-explorer";
pub const MCP: &str = "bmo-explorer mcp";

/// What a harness does first with it, which the server tells a harness when it starts.
pub const ENTRY: &str =
  "A harness searches with explorer_web_search and reads a hit with explorer_web_fetch.";

/// What a client function's call comes to: its answer, or the failure in the wire format's error shape.
pub type Answering<'a, O> = Pin<Box<dyn Future<Output = Result<O, ClientError>> + Send + 'a>>;

type Call = Box<dyn for<'a> Fn(&'a Explorer, Value) -> Answering<'a, Value> + Send + Sync>;

/// One tool: its name, what it does, the client function it calls, its input and output, and the call itself.
pub struct ExplorerTool {
  pub name: &'static str,
  pub description: &'static str,
  /// The client function the tool calls.
  pub function: &'static str,
  input: fn(&mut SchemaGenerator) -> Schema,
  output: fn(&mut SchemaGenerator) -> Schema,
  call: Call,
}

impl ExplorerTool {
  /// The tool's input, its nested types referred to where `generator` keeps its definitions.
  pub fn input(&self, generator: &mut SchemaGenerator) -> Schema {
    (self.input)(generator)
  }

  /// The tool's output, referred to where `generator` keeps its definitions.
  pub fn output(&self, generator: &mut SchemaGenerator) -> Schema {
    (self.output)(generator)
  }

  /// Calls the tool's client function with the tool's arguments.
  pub fn call<'a>(&self, client: &'a Explorer, arguments: Value) -> Answering<'a, Value> {
    (self.call)(client, arguments)
  }
}

/// A tool whose input is `I` and whose answer is `O`, calling `call` with the arguments read as `I`.
fn tool<I, O>(
  name: &'static str,
  description: &'static str,
  function: &'static str,
  call: for<'a> fn(&'a Explorer, I) -> Answering<'a, O>,
) -> ExplorerTool
where
  I: DeserializeOwned + JsonSchema + 'static,
  O: Serialize + JsonSchema + 'static,
{
  ExplorerTool {
    name,
    description,
    function,
    input: I::json_schema,
    output: SchemaGenerator::subschema_for::<O>,
    call: Box::new(move |client, arguments| match serde_json::from_value::<I>(arguments) {
      Ok(input) => {
        let answering = call(client, input);

        Box::pin(async move {
          let answer = answering.await?;

          serde_json::to_value(answer)
            .map_err(|why| ClientError::new("failed", format!("The answer could not be written: {why}.")))
        })
      }
      Err(why) => {
        let failure = ClientError::new(
          "bad-request",
          format!("The arguments are not the shape this tool takes: {why}."),
        );

        Box::pin(std::future::ready(Err(failure)))
      }
    }),
  }
}

fn web_search(client: &Explorer, asked: WebSearchInput) -> Answering<'_, WebSearchOutput> {
  Box::pin(async move { client.web_search(&asked).await })
}

fn web_fetch(client: &Explorer, asked: UrlInput) -> Answering<'_, PageText> {
  Box::pin(async move { client.web_fetch(&asked.url).await })
}

fn link_preview(client: &Explorer, asked: UrlInput) -> Answering<'_, LinkPreview> {
  Box::pin(async move { client.link_preview(&asked.url).await })
}

fn fetch_page(client: &Explorer, asked: UrlInput) -> Answering<'_, PageSource> {
  Box::pin(async move { client.fetch_page(&asked.url).await })
}

fn fetch_raster(client: &Explorer, asked: RasterInput) -> Answering<'_, Raster> {
  Box::pin(async move { client.fetch_raster(&asked).await })
}

fn search_engines(client: &Explorer, _: NoInput) -> Answering<'_, SearchEnginesOutput> {
  Box::pin(async move { client.search_engines().await })
}

/// Every tool the server lists, in the order the OpenAPI document states them.
pub fn tools() -> Vec<ExplorerTool> {
  vec![
    tool(
      "explorer_web_search",
      "Ask the search engines directly; the hits, as each engine answered them, mixed one from each engine in \
       turn. Name the engines or their categories from explorer_search_engines, or none for the general ones.",
      "web_search",
      web_search,
    ),
    tool(
      "explorer_web_fetch",
      "Read one page as words: its title, the text a reader sees and the addresses it links to. A page that \
       blocks a plain request is read in a real browser; a PDF comes back as its text.",
      "web_fetch",
      web_fetch,
    ),
    tool(
      "explorer_link_preview",
      "A link's preview as the page's head declares it: title, description, picture.",
      "link_preview",
      link_preview,
    ),
    tool(
      "explorer_fetch_page",
      "Fetch a page's HTML as served, with its status and content type: at most 512 KiB, decoded as its \
       charset says.",
      "fetch_page",
      fetch_page,
    ),
    tool(
      "explorer_fetch_raster",
      "The picture at an address, base64 encoded with its type; narrowed to a width as JPEG where one is named.",
      "fetch_raster",
      fetch_raster,
    ),
    tool(
      "explorer_search_engines",
      "The engines that answer, and their standing: each engine's category and state as its last request left \
       it, and how long an engine rests after each kind of refusal.",
      "search_engines",
      search_engines,
    ),
  ]
}

/// The generator every tool schema and the OpenAPI document are written with: JSON Schema 2020-12, read as a
/// caller sends it, definitions kept at `definitions_path`.
pub fn generator(definitions_path: &'static str) -> SchemaGenerator {
  SchemaSettings::draft2020_12()
    .for_deserialize()
    .with(|settings| {
      settings.definitions_path = definitions_path.into();
      settings.meta_schema = None;
    })
    .into_generator()
}

/// The server over one client.
pub struct ExplorerServer {
  client: Arc<Explorer>,
  tools: Arc<Vec<ExplorerTool>>,
}

impl ExplorerServer {
  pub fn new(client: Arc<Explorer>) -> Self {
    Self {
      client,
      tools: Arc::new(tools()),
    }
  }

  /// The tools as MCP lists them, each input and output a schema of its own with its definitions inside.
  pub fn listed(&self) -> Vec<Tool> {
    self.tools.iter().map(listed).collect()
  }
}

fn listed(tool: &ExplorerTool) -> Tool {
  let mut generator = generator("/$defs");
  let input = tool.input(&mut generator);
  let output = tool.output(&mut generator);
  let definitions = generator.take_definitions(true);
  let mut listed = Tool::new(tool.name, tool.description, standing_alone(input, &definitions));
  // Each reads the web and changes nothing on this machine, which lets a harness send a turn's calls side by
  // side.
  let mut annotations = ToolAnnotations::default();

  annotations.read_only_hint = Some(true);
  annotations.destructive_hint = Some(false);
  annotations.open_world_hint = Some(true);
  listed.output_schema = Some(Arc::new(standing_alone(output, &definitions)));
  listed.annotations = Some(annotations);
  listed
}

/// A schema made to stand on its own: an object at its root, with every definition it refers to beside it.
fn standing_alone(schema: Schema, definitions: &Map<String, Value>) -> JsonObject {
  let mut object = match schema.to_value() {
    Value::Object(object) if object.contains_key("$ref") => {
      let mut wrapped = Map::new();

      wrapped.insert("allOf".into(), Value::Array(vec![Value::Object(object)]));
      wrapped.insert("type".into(), "object".into());
      wrapped
    }
    Value::Object(object) => object,
    _ => Map::from_iter([("type".to_owned(), Value::from("object"))]),
  };

  if !definitions.is_empty() {
    object.insert("$defs".into(), Value::Object(definitions.clone()));
  }

  object
}

impl ServerHandler for ExplorerServer {
  fn get_info(&self) -> InitializeResult {
    let mut info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build());
    let mut server = Implementation::default();

    server.name = "bmo-explorer".to_owned();
    server.version = crate::VERSION.to_owned();
    info.server_info = server;
    info.instructions = Some(ENTRY.to_owned());
    info
  }

  async fn list_tools(
    &self,
    _: Option<PaginatedRequestParams>,
    _: RequestContext<RoleServer>,
  ) -> Result<ListToolsResult, ErrorData> {
    Ok(ListToolsResult::with_all_items(self.listed()))
  }

  async fn call_tool(
    &self,
    request: CallToolRequestParams,
    _: RequestContext<RoleServer>,
  ) -> Result<CallToolResponse, ErrorData> {
    let Some(tool) = self.tools.iter().find(|tool| tool.name == request.name) else {
      return Err(ErrorData::invalid_params(format!("There is no tool {}.", request.name), None));
    };
    let arguments = Value::Object(request.arguments.unwrap_or_default());
    let result = match tool.call(&self.client, arguments).await {
      Ok(answer) => CallToolResult::structured(answer),
      Err(failure) => CallToolResult::error(vec![ContentBlock::text(
        serde_json::to_string(&failure).unwrap_or_else(|_| failure.to_string()),
      )]),
    };

    Ok(CallToolResponse::Complete(result))
  }
}
