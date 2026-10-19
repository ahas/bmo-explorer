use bmo_explorer::site::readers::markup::{page_one, parse_html};
use bmo_explorer::site::readers::plain::*;

#[test]
fn a_fragment_reads_with_its_links_bare_its_spans_joined_and_its_parts_left_out() {
  let page = parse_html(
    "<li><a href=\"/p\">상품</a> <span>12,000</span><span>원</span><dl><dt>a</dt></dl><p>끝</p></li>",
  );
  let item = page_one(&page, "li").unwrap();

  assert_eq!(readable([*item], &Plain::default()), "상품 (/p) 12,000원a\n\n끝");
  assert_eq!(
    one_line(
      [*item],
      &Plain {
        bare_links: true,
        span_joint: Some(" "),
        leave_out: &["dl"],
      }
    ),
    "상품 12,000 원끝"
  );
}
