use bmo_explorer::web::identity::*;

#[test]
fn every_identity_is_one_chrome_of_the_installed_version_and_says_the_same_platform_twice() {
  assert_eq!(IDENTITIES.map(|one| one.name), ["linux", "windows", "macos", "android"]);

  for identity in IDENTITIES {
    let headers = navigation_headers(FetchSite::None, identity);
    let header = |name: &str| headers.iter().find(|(key, _)| *key == name).unwrap().1.clone();
    let platform_word = match identity.platform {
      "\"Linux\"" => "X11; Linux",
      "\"Windows\"" => "Windows NT",
      "\"macOS\"" => "Macintosh",
      _ => "Android",
    };

    assert_eq!(header("user-agent"), identity.user_agent);
    assert!(identity.user_agent.contains(&format!("Chrome/{CHROME_MAJOR}.0.0.0")));
    assert!(header("sec-ch-ua").contains(&format!("v=\"{CHROME_MAJOR}\"")));
    assert!(identity.user_agent.contains(platform_word));
    assert_eq!(header("sec-ch-ua-mobile"), if identity.mobile { "?1" } else { "?0" });
    assert_eq!(identity.user_agent.contains("Mobile"), identity.mobile);
    assert!(!identity.user_agent.to_lowercase().contains("bot"));
  }

  assert_eq!(named(Some("android")).name, "android");
  assert_eq!(named(Some("not-an-identity")).name, "linux");
  assert_eq!(named(None).name, "linux");
  assert_eq!(next(named(Some("linux"))).name, "windows");
  assert_eq!(next(named(Some("android"))).name, "linux");
}

#[test]
fn a_previewer_says_it_is_one_and_sends_no_browser_s_hints() {
  let headers = Presentation::Previewer.headers(FetchSite::CrossSite);
  let names: Vec<&str> = headers.iter().map(|(name, _)| *name).collect();

  assert_eq!(names, ["accept", "accept-encoding", "accept-language", "user-agent"]);
  assert!(headers.contains(&("user-agent", PREVIEWER_AGENT.to_owned())));
  assert!(PREVIEWER_AGENT.starts_with("facebookexternalhit/1.1"));
  assert_eq!(
    Presentation::Browser(DEFAULT).headers(FetchSite::None),
    navigation_headers(FetchSite::None, DEFAULT)
  );
}
