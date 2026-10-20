//! A site tool served as a placed entry (docs/site-contract.md, 「Where a tool lives」): its calls reach the site
//! through the record's web helper, under the research guard the helper keeps, and its claims are answered from
//! the trait. What the host reads of it without running it — its listing line, its catalogue item and the hosts
//! it claims — stands in its manifest, held to the trait here.

use crate::tool::manifest::Manifest;
use crate::tool::{Answer, Hints, Served, Session, Tool};
use serde_json::{Value, json};
use url::Url;

use crate::site::contract::SiteTool;
use crate::site::helper::HelperReach;

/// Every site tool changes nothing on this machine and says so: Claude Code sends the calls of an MCP tool side
/// by side only when the tool is marked read-only, and one at a time otherwise, which would run a turn's
/// readings of its sites in series.
pub const REACHING: Hints = Hints {
  read_only: true,
  destructive: false,
  idempotent: None,
  open_world: true,
};

/// The facts list of the web namespace a site tool adds itself to.
pub const CATALOGUE: &str = "sites";

/// A site tool as its placed executable serves it.
pub struct Placed<T>(pub T);

impl<T: SiteTool> Tool for Placed<T> {
  const NAME: &'static str = T::NAME;
  const NAMESPACE: &'static str = "web";
  const TITLE: Option<&'static str> = Some(T::TITLE);
  const DESCRIPTION: &'static str = T::DESCRIPTION;
  const HINTS: Hints = REACHING;
  // The site tools are the searcher's alone (the tool host's docs/toolbook.md, 「Each line's toolbook」).
  const CAPABILITIES: &'static [&'static str] = &["web.sites"];

  type Input = T::Input;

  fn claim(address: &str) -> Option<Value> {
    T::claim(&Url::parse(address).ok()?)
  }

  async fn call(&self, session: &Session, input: T::Input) -> Answer {
    let reach = HelperReach::of(session);

    match reach.opening(T::ENGINE).await {
      Ok(None) => {}
      Ok(Some(refused)) => return Answer::text(refused),
      Err(why) => return Answer::failure(why),
    }

    match self.0.run(input, &reach).await {
      Ok(text) if !T::ENGINE => Answer::text(text),
      outcome => reach.ended(T::ENGINE, outcome).await,
    }
  }
}

/// A site tool's line in the web listing: its name, what it fetches, the site, since when, and the addresses it
/// claims.
pub fn listing_line(name: &str, what: &str, site: &str, since: &str, forms: Option<&str>) -> String {
  let answers = forms.map(|forms| format!(" · answers {forms}")).unwrap_or_default();

  format!("- {name} — {what} · {site} · since {since}{answers}")
}

/// A site tool's place in the listing, the oldest reading first: the date it was made, read as its digits.
pub fn listing_order(since: &str) -> i64 {
  since.chars().filter(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)
}

/// The sites a tool reads: the hosts its site line and its claimed forms name.
pub fn sites_read(site: &str, forms: Option<&str>) -> Vec<String> {
  crate::site::sites::sites_named_in(&format!("{site} {}", forms.unwrap_or("")))
}

/// The hosts a placed site tool's manifest claims: those its forms name, then those its trait adds.
pub fn claimed_hosts<T: SiteTool>() -> Vec<String> {
  let mut hosts = T::FORMS.map(|forms| sites_read("", Some(forms))).unwrap_or_default();

  for host in T::ALSO_CLAIMS {
    if !hosts.iter().any(|named| named == host) {
      hosts.push((*host).to_owned());
    }
  }

  hosts
}

/// The entry of a tool's own test crate that claims an address, with the arguments it reads the page with, asked
/// as the host asks: the entries its manifest says claim the address's host, in turn, the first that claims it
/// answering. The manifest is the one in the folder whose test runs, which Cargo names in `CARGO_MANIFEST_DIR`.
pub fn claiming(tools: &'static [&'static dyn Served], url: &Url) -> Option<(&'static dyn Served, Value)> {
  let dir =
    std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("a test run names its folder"));
  let folder = dir.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_owned();
  let manifest = Manifest::read(&dir, &folder).unwrap_or_else(|why| panic!("{}: {why}", dir.display()));

  manifest.entries.iter().filter(|entry| entry.claims(url.as_str())).find_map(|entry| {
    let tool = tools
      .iter()
      .find(|tool| format!("{}/{}", tool.namespace(), tool.name()) == entry.full_name())?;

    tool.claim(url.as_str()).map(|arguments| (*tool, arguments))
  })
}

/// Whether a site tool reads a site, for the list of pages no road read: it claims the address itself, or its
/// site line or claimed forms name the site.
pub fn reads<T: SiteTool>(site: &str, url: Option<&str>) -> bool {
  url.and_then(|url| Url::parse(url).ok()).is_some_and(|url| T::claim(&url).is_some())
    || sites_read(T::SITE, T::FORMS).iter().any(|named| named == site)
}

/// A site tool's item in the web namespace's site catalogue, less the name the host heads it with: what it
/// fetches, the site, when it was made, its arguments, the addresses it claims, its description and the sites
/// it reads.
pub fn catalogue_item(
  what: &str,
  site: &str,
  since: &str,
  input_schema: &Value,
  forms: Option<&str>,
  description: &str,
) -> Value {
  let arguments: Vec<&String> = input_schema["properties"]
    .as_object()
    .map(|fields| fields.keys().collect())
    .unwrap_or_default();

  json!({
    "what": what,
    "site": site,
    "since": since,
    "arguments": arguments,
    "answers": forms,
    "description": description,
    "reads": sites_read(site, forms),
  })
}

/// Where a placed site tool's manifest says otherwise than its tool of what the host reads without running it:
/// its listing line and order, its catalogue item and the hosts it claims, which are those its forms name. A
/// tool's own test asks it beside `Manifest::differences`.
pub fn declared_differences<T: SiteTool>(manifest: &Manifest) -> Vec<String> {
  let at = format!("web/{}", T::NAME);
  let Some(entry) = manifest.entries.iter().find(|entry| entry.full_name() == at) else {
    return vec![format!("{at}: no entry declares it")];
  };
  let line = listing_line(T::NAME, T::WHAT, T::SITE, T::SINCE, T::FORMS);
  let order = listing_order(T::SINCE);
  // The entry's schema is the one `Manifest::differences` holds to the tool's input type.
  let mut item = catalogue_item(T::WHAT, T::SITE, T::SINCE, &entry.input_schema, T::FORMS, T::DESCRIPTION);
  let mut differ = Vec::new();

  if entry.listing.as_ref().map(|listing| (listing.line.as_str(), listing.order)) != Some((&*line, order)) {
    differ.push(format!(
      "{at}: its listing differs from its tool's, which is line = {line:?}, order = {order}"
    ));
  }

  let declared = entry
    .facts
    .as_ref()
    .and_then(|facts| facts.lists.get(CATALOGUE))
    .cloned()
    .map(Value::Object);

  if let Some(fields) = item.as_object_mut() {
    fields.retain(|_, value| !value.is_null());
  }

  if declared.as_ref() != Some(&item) {
    differ.push(format!(
      "{at}: its [entry.facts.lists.{CATALOGUE}] differs from its tool's, which is {item}"
    ));
  }

  let hosts = claimed_hosts::<T>();
  let claimed = entry.claims.as_ref().map(|claims| claims.hosts.clone()).unwrap_or_default();

  if claimed != hosts {
    differ.push(format!(
      "{at}: its claimed hosts differ from those its forms name and its trait adds, which are {hosts:?}"
    ));
  }

  differ
}
