use bmo_explorer::site::readers::markup::*;

#[test]
fn a_members_markup_reads_as_the_blocks_a_reader_sees() {
  let text = markup_text(
    "<p>Hello <a href=\"/t/1\">topic</a> and <a href=\"https://x.example/a/very/long\">https://x.example/a/very...</a></p><pre>code\n</pre><blockquote><p>quoted</p></blockquote><ul><li>one</li><li>two</li></ul><table><tr><th>a</th><td>b</td></tr></table><hr><script>no()</script>",
    "https://forum.example",
  );

  assert_eq!(
    text,
    "Hello topic (https://forum.example/t/1) and https://x.example/a/very/long\n\n```\ncode\n```\n\n> quoted\n\n- one\n- two\n\na | b\n\n---"
  );
}

#[test]
fn a_table_whose_headings_span_columns_names_each_figure_by_the_headings_over_it() {
  let page = parse_html(
    "<table><thead><tr><th rowspan=\"2\">국가</th><th colspan=\"2\">2024</th></tr><tr><th>입국</th><th>출국</th></tr></thead><tbody><tr><th>일본</th><td>10</td><td>20</td></tr></tbody></table>",
  );
  let table = page_one(&page, "table").unwrap();

  assert_eq!(table_lines(table), ["일본: 2024 입국 10 · 2024 출국 20"]);
}
