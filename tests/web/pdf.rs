use bmo_explorer::web::error::Failure;
use bmo_explorer::web::pdf::*;

use crate::testing::pdf_of;

// Carried from packages/web/pdf-text.test.ts: knows a PDF by its type or, sent as anything else, by its signature
#[test]
fn a_pdf_is_known_by_its_type_or_its_signature() {
  assert!(is_pdf("application/pdf", b""));
  assert!(is_pdf("application/octet-stream", "%PDF-1.6\n…".as_bytes()));
  assert!(!is_pdf("text/html", b"<html>%PDF-</html>"));
  assert!(!is_pdf("application/octet-stream", b""));
}

// Carried from packages/web/pdf-text.test.ts: heads each page with its number and drops the form feed that ends the last
#[test]
fn each_page_is_headed_by_its_number() {
  assert_eq!(
    paged_text("Rated pressure   \n\n\n\n-100 kPa\u{c}Wiring\u{c}"),
    "[page 1]\nRated pressure\n\n-100 kPa\n\n[page 2]\nWiring"
  );
}

// Carried from packages/web/pdf-text.test.ts: is empty where no page holds text
#[test]
fn a_reading_with_no_text_is_empty() {
  assert_eq!(paged_text("\u{c}\u{c}"), "");
}

// Carried from packages/web/pdf-text.test.ts: reads each page's text in order
#[tokio::test]
async fn each_pages_text_is_read_in_order() {
  let text = pdf_text(
    &pdf_of(&["MPS-35 Series", "Output Voltage: 1 to 5V"]),
    "https://maker.test/manual.pdf",
    false,
    false,
  )
  .await
  .unwrap();

  assert_eq!(text, "[page 1]\nMPS-35 Series\n\n[page 2]\nOutput Voltage: 1 to 5V");
}

// Carried from packages/web/pdf-text.test.ts: says a PDF with no text layer holds pictures, never answering blank
#[tokio::test]
async fn a_pdf_with_no_text_layer_says_it_holds_pictures() {
  assert_eq!(
    pdf_text(&pdf_of(&[""]), "https://maker.test/scan.pdf", false, false).await.unwrap(),
    NO_PDF_TEXT
  );
}

// Carried from packages/web/pdf-text.test.ts: names a file the reader cannot open as the site's, and a cut one as this machine's limit
#[tokio::test]
async fn an_unreadable_file_is_the_sites_and_a_cut_one_this_machines_limit() {
  let broken = b"%PDF-1.4\nnot a document";

  assert_eq!(
    pdf_text(broken, "https://maker.test/broken.pdf", false, false).await.unwrap_err().kind,
    Failure::Transport
  );
  assert_eq!(
    pdf_text(broken, "https://maker.test/cut.pdf", true, false).await.unwrap_err().kind,
    Failure::TooLarge
  );
}
