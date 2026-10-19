//! The browsers a direct read speaks as (docs/web-access.md, 「Identities and headers」): each one stable
//! Chrome of the installed major version, whose user agent and three client hints are one statement about
//! one browser and are only ever set together.

/// The installed Chrome's major version, which every identity names.
pub const CHROME_MAJOR: u32 = 151;

/// One stable Chrome as a site sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity {
  /// The name a recipe keeps, so an engine asks as the same browser from one run to the next.
  pub name: &'static str,
  pub user_agent: &'static str,
  /// `sec-ch-ua-platform`, quoted as the header carries it.
  pub platform: &'static str,
  pub mobile: bool,
}

pub const IDENTITIES: [Identity; 4] = [
  Identity {
    name: "linux",
    user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36",
    platform: "\"Linux\"",
    mobile: false,
  },
  Identity {
    name: "windows",
    user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36",
    platform: "\"Windows\"",
    mobile: false,
  },
  Identity {
    name: "macos",
    user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Safari/537.36",
    platform: "\"macOS\"",
    mobile: false,
  },
  Identity {
    name: "android",
    user_agent: "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/151.0.0.0 Mobile Safari/537.36",
    platform: "\"Android\"",
    mobile: true,
  },
];

/// Page fetches ask as the machine the web layer runs on.
pub const DEFAULT: Identity = IDENTITIES[0];

/// The identity a recipe names, or the default where it names none this code knows.
pub fn named(name: Option<&str>) -> Identity {
  IDENTITIES.into_iter().find(|one| Some(one.name) == name).unwrap_or(DEFAULT)
}

/// The identity after this one, round the list: what an engine moves to when a site reads the browser it
/// asked as and refuses it.
pub fn next(current: Identity) -> Identity {
  let at = IDENTITIES.iter().position(|one| one.name == current.name).unwrap_or(0);

  IDENTITIES[(at + 1) % IDENTITIES.len()]
}

/// How a navigation stands to the one before it, as `Sec-Fetch-Site` says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchSite {
  None,
  SameOrigin,
  SameSite,
  CrossSite,
}

impl FetchSite {
  pub fn header(self) -> &'static str {
    match self {
      Self::None => "none",
      Self::SameOrigin => "same-origin",
      Self::SameSite => "same-site",
      Self::CrossSite => "cross-site",
    }
  }
}

/// The headers a top-level navigation of this browser sends, by their lower-case names.
pub fn navigation_headers(site: FetchSite, identity: Identity) -> Vec<(&'static str, String)> {
  vec![
    (
      "accept",
      "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7".into(),
    ),
    ("accept-encoding", "gzip, deflate, br, zstd".into()),
    ("accept-language", "ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7".into()),
    ("cache-control", "max-age=0".into()),
    ("priority", "u=0, i".into()),
    (
      "sec-ch-ua",
      format!("\"Google Chrome\";v=\"{CHROME_MAJOR}\", \"Chromium\";v=\"{CHROME_MAJOR}\", \"Not_A Brand\";v=\"24\""),
    ),
    ("sec-ch-ua-mobile", if identity.mobile { "?1" } else { "?0" }.into()),
    ("sec-ch-ua-platform", identity.platform.into()),
    ("sec-fetch-dest", "document".into()),
    ("sec-fetch-mode", "navigate".into()),
    ("sec-fetch-site", site.header().into()),
    ("sec-fetch-user", "?1".into()),
    ("upgrade-insecure-requests", "1".into()),
    ("user-agent", identity.user_agent.into()),
  ]
}
