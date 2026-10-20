//! The sites an address or a line of words names (docs/site-contract.md, 「A page a tool claims」): an
//! address's site, and the hosts a site tool's site line and claimed forms name.

use std::sync::LazyLock;

use regex::Regex;

/// The site an address belongs to: its host, lower-cased, with a leading `www.`, `m.` or `mobile.` dropped so
/// a site's desktop and mobile pages are one site. None for what is not an address.
pub fn site_of(address: &str) -> Option<String> {
  let host = url::Url::parse(address).ok()?.host_str()?.to_lowercase();

  Some(
    ["www.", "m.", "mobile."]
      .iter()
      .find_map(|prefix| host.strip_prefix(prefix))
      .unwrap_or(&host)
      .to_owned(),
  )
}

static HOST: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z]{2,}").expect("a fixed pattern")
});

/// The hosts a line of words names, as sites: `Example Finance (m.finance.example.com)` names finance.example.com.
pub fn sites_named_in(text: &str) -> Vec<String> {
  let lower = text.to_lowercase();
  let mut sites: Vec<String> = Vec::new();

  for found in HOST.find_iter(&lower) {
    // A host stands apart from the words around it: no word character, dot or hyphen touching either side, and
    // no single slash before it, which makes it a path's segment — `gasazip.com/search.html` names one site.
    let before = lower[..found.start()].chars().next_back();
    let after = lower[found.end()..].chars().next();
    let apart = |c: Option<char>, dot: bool| {
      c.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-' || (dot && c == '.')))
    };
    let in_path = before == Some('/') && !lower[..found.start()].ends_with("//");

    if !apart(before, true) || !apart(after, false) || in_path {
      continue;
    }

    if let Some(site) = site_of(&format!("https://{}/", found.as_str()))
      && !sites.contains(&site)
    {
      sites.push(site);
    }
  }

  sites
}
