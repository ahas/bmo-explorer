use bmo_explorer::forms::slice::*;

// Carried from packages/web/web.test.ts: cuts at the asked size and names where the rest starts
#[test]
fn a_slice_cuts_at_the_asked_size_and_names_where_the_rest_starts() {
  let slice = slice_content("https://a.io", "text/plain", "0123456789", 3, 4);

  assert_eq!(
    slice,
    FetchSlice {
      url: "https://a.io".into(),
      content_type: "text/plain".into(),
      status: 200,
      provenance: "direct",
      source_truncated: false,
      total: 10,
      offset: 3,
      text: "3456".into(),
      next_offset: Some(7),
    }
  );

  let rendered = render_slice(&slice);

  assert!(rendered.contains("CHARS: 3-7 of 10 (more: offset 7)"));
  assert!(rendered.contains("FETCHED-WITH: direct"));
  assert!(rendered.contains("SOURCE-READ: complete"));
  assert_eq!(slice_content("https://a.io", "text/plain", "0123", 0, 100).next_offset, None);
}
