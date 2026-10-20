use bmo_explorer::site::sites::*;

// Carried from packages/web/read-failures.test.ts: is the host with its www, m or mobile prefix dropped
#[test]
fn a_site_is_its_host_with_its_www_m_or_mobile_prefix_dropped() {
  assert_eq!(site_of("https://namu.wiki/w/Vampire%20Survivors").as_deref(), Some("namu.wiki"));
  assert_eq!(
    site_of("https://m.Stock.Naver.com/domestic/stock/005930").as_deref(),
    Some("stock.naver.com")
  );
  assert_eq!(site_of("https://www.example.com/a").as_deref(), Some("example.com"));
  assert_eq!(site_of("not an address"), None);
}

// Carried from packages/web/read-failures.test.ts: is read out of a tool's site line by the hosts it names
#[test]
fn a_tools_site_line_names_its_sites_by_their_hosts() {
  assert_eq!(
    sites_named_in(
      "네이버 증권 (m.stock.naver.com, api.stock.naver.com) and WiseReport (navercomp.wisereport.co.kr)"
    ),
    ["stock.naver.com", "api.stock.naver.com", "navercomp.wisereport.co.kr"]
  );
  assert!(sites_named_in("나무위키").is_empty());
  assert_eq!(
    sites_named_in("가사집 (gasazip.com) gasazip.com/<number> and https://www.gasazip.com/search.html?q=…"),
    ["gasazip.com"]
  );
}
