//! The client's tests (docs/api.md, 「The client」): each operation over the stand-in web, and the failures it
//! answers in the wire format's error shape.

use std::io::Cursor;
use std::sync::Arc;

use base64::Engine;
use bmo_explorer::client::{Explorer, ImageSearchInput, RasterInput, WebSearchInput};
use bmo_explorer::web::chrome::{BrowserDocument, BrowserFetcher};
use bmo_explorer::web::clock::StandingClock;
use bmo_explorer::web::pacing::MemoryPacing;
use bmo_explorer::web::session::{SessionParts, WebSession};

use crate::web::testing::{FakeBrowser, Web, answer, check, fixture, html};

// A page as the stand-in Chrome renders every address it is sent to.
fn rendered() -> BrowserDocument {
  BrowserDocument {
    url: "https://shop.test/item".into(),
    status: 200,
    content_type: "text/html".into(),
    title: "Item".into(),
    text: "drawn by script".into(),
    html: "<html><head><title>Item</title></head><body><p>drawn by script</p><a href=\"/cart\">Cart</a></body></html>"
      .into(),
    ..BrowserDocument::default()
  }
}

fn client(web: Web) -> (Explorer, Arc<Web>, Arc<FakeBrowser>) {
  let web = Arc::new(web);
  let browser = FakeBrowser::new(rendered());
  let made: Arc<dyn BrowserFetcher> = browser.clone();
  let client = Explorer::over(WebSession::new(SessionParts {
    store: Arc::new(MemoryPacing::default()),
    transport: web.clone(),
    check: check(StandingClock::at(1_000_000)),
    host_interval_ms: 0,
    browsers: Some(Arc::new(move |_| made.clone())),
  }));

  (client, web, browser)
}

const ARTICLE: &str = r#"<html><head><title> An  article </title>
<meta property="og:title" content="The article, as shared">
<meta name="description" content="What it says, in a line.">
<meta property="og:image" content="/pictures/cover.png">
</head><body><h1>Heading</h1><p>The body's words.</p>
<a href="/next#part">Next</a> <a href="https://other.test/page">Elsewhere</a> <a href="/next">Again</a>
<a href="mailto:someone@example.org">Mail</a></body></html>"#;

#[tokio::test]
async fn a_page_is_read_as_its_title_its_words_and_the_addresses_it_links_to() {
  let (client, ..) = client(Web::default().route("site.test", |_| html(ARTICLE)));
  let page = client.web_fetch("https://site.test/article").await.unwrap();

  assert_eq!(page.url, "https://site.test/article");
  assert_eq!(page.title.as_deref(), Some("An article"));
  assert!(
    page.text.contains("Heading") && page.text.contains("The body's words."),
    "{}",
    page.text
  );
  assert!(!page.text.contains("<p>"));
  assert_eq!(page.links, ["https://site.test/next", "https://other.test/page"]);
  assert!(!page.truncated);
}

#[tokio::test]
async fn a_page_that_blocks_a_plain_request_is_read_as_chrome_drew_it() {
  let (client, _, browser) = client(
    Web::default().route("shop.test", |_| answer(403, &[("content-type", "text/html")], b"<p>denied</p>")),
  );
  let page = client.web_fetch("https://shop.test/item").await.unwrap();

  assert_eq!(browser.calls(), 1);
  assert_eq!(page.title.as_deref(), Some("Item"));
  assert!(page.text.contains("drawn by script"), "{}", page.text);
  assert_eq!(page.links, ["https://shop.test/cart"]);
}

#[tokio::test]
async fn a_preview_is_what_the_head_declares_for_one() {
  let (client, ..) = client(Web::default().route("site.test", |_| html(ARTICLE)));
  let preview = client.link_preview("https://site.test/article").await.unwrap();

  assert_eq!(preview.title.as_deref(), Some("The article, as shared"));
  assert_eq!(preview.description.as_deref(), Some("What it says, in a line."));
  assert_eq!(preview.image.as_deref(), Some("https://site.test/pictures/cover.png"));
}

#[tokio::test]
async fn a_page_s_markup_is_answered_with_its_status_and_a_file_that_is_not_markup_is_refused() {
  let (client, ..) = client(
    Web::default()
      .route("site.test", |_| {
        answer(404, &[("content-type", "text/html; charset=utf-8")], b"<p>gone</p>")
      })
      .route("files.test", |_| answer(200, &[("content-type", "application/pdf")], b"%PDF-1.7")),
  );
  let page = client.fetch_page("https://site.test/missing").await.unwrap();
  let file = client.fetch_page("https://files.test/paper.pdf").await.unwrap_err();

  assert_eq!((page.status, page.html.as_str()), (404, "<p>gone</p>"));
  assert_eq!(page.content_type, "text/html; charset=utf-8");
  assert_eq!(file.code, "not-markup");
}

#[tokio::test]
async fn an_address_inside_this_network_is_refused_before_anything_is_asked() {
  let (client, web, _) = client(Web::default());
  let refused = client.fetch_page("http://127.0.0.1/admin").await.unwrap_err();

  assert_eq!(refused.code, "unsafe-address");
  assert!(web.asked().is_empty());
}

fn png(width: u32, height: u32) -> Vec<u8> {
  let mut out = Vec::new();

  image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(width, height, image::Rgb([200, 40, 40])))
    .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
    .unwrap();
  out
}

#[tokio::test]
async fn a_picture_wider_than_asked_is_redrawn_that_wide_and_any_other_is_answered_as_served() {
  let picture = png(400, 200);
  let served = picture.clone();
  let (client, ..) = client(
    Web::default().route("pictures.test", move |_| answer(200, &[("content-type", "image/png")], &served)),
  );
  let asked = |width| RasterInput {
    url: "https://pictures.test/a.png".into(),
    width,
  };
  let narrowed = client.fetch_raster(&asked(Some(100))).await.unwrap();
  let whole = client.fetch_raster(&asked(Some(1_000))).await.unwrap();
  let zero = client.fetch_raster(&asked(Some(0))).await.unwrap_err();
  let bytes = |image: &str| base64::engine::general_purpose::STANDARD.decode(image).unwrap();
  let redrawn = image::load_from_memory(&bytes(&narrowed.image)).unwrap();

  assert_eq!(narrowed.mime_type, "image/jpeg");
  assert_eq!((redrawn.width(), redrawn.height()), (100, 50));
  assert_eq!((whole.mime_type.as_str(), bytes(&whole.image)), ("image/png", picture));
  assert_eq!(zero.code, "bad-request");
}

#[tokio::test]
async fn a_search_asks_only_the_engines_it_names_and_refuses_a_name_no_roster_holds() {
  let (client, web, _) = client(Web::default());
  let search = |engines: &[&str]| WebSearchInput {
    q: "rust async".into(),
    engines: engines.iter().map(|name| (*name).to_owned()).collect(),
    ..WebSearchInput::default()
  };
  let found = client.web_search(&search(&["github"])).await.unwrap();
  let unknown = client.web_search(&search(&["altavista"])).await.unwrap_err();
  let empty = client
    .web_search(&WebSearchInput {
      q: "  ".into(),
      ..WebSearchInput::default()
    })
    .await
    .unwrap_err();

  assert!(found.hits.is_empty());
  assert!(
    found.unresponsive.iter().all(|engine| engine.starts_with("github")),
    "{:?}",
    found.unresponsive
  );
  assert!(!web.asked().is_empty() && web.asked().iter().all(|asked| asked.host.contains("github")));
  assert_eq!((unknown.code.as_str(), empty.code.as_str()), ("bad-request", "bad-request"));
}

#[tokio::test]
async fn an_image_search_asks_only_image_engines_and_answers_their_fields() {
  let openverse = fixture("engines/openverse.json");
  let (client, web, _) = client(Web::default().route("api.openverse.org", move |_| {
    answer(200, &[("content-type", "application/json")], openverse.as_bytes())
  }));
  let asked = |engines: &[&str]| ImageSearchInput {
    q: "cherry".into(),
    engines: engines.iter().map(|name| (*name).to_owned()).collect(),
    ..ImageSearchInput::default()
  };
  let found = client.search_images(&asked(&["openverse"])).await.unwrap();
  let general = client.search_images(&asked(&["general"])).await.unwrap_err();
  let unknown = client.search_images(&asked(&["altavista"])).await.unwrap_err();
  let empty = client
    .search_images(&ImageSearchInput {
      q: "  ".into(),
      ..ImageSearchInput::default()
    })
    .await
    .unwrap_err();

  assert_eq!(found.images.len(), 2);
  assert_eq!(found.images[0].engine, "openverse");
  assert_eq!(found.images[0].licence.as_ref().unwrap().id, "CC-BY-NC-2.0");
  assert_eq!((found.images[0].width, found.images[0].height), (Some(500), Some(375)));
  assert!(found.images[0].media_type.is_none());
  assert!(web.asked().iter().all(|one| one.host == "api.openverse.org"));
  assert_eq!(
    (general.code.as_str(), unknown.code.as_str(), empty.code.as_str()),
    ("bad-request", "bad-request", "bad-request")
  );
}
