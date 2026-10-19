use bmo_explorer::site::readers::ordered::*;

#[test]
fn an_objects_fields_keep_the_order_a_script_walks_them() {
  let tree: Tree = serde_json::from_str(r#"{"b":1,"a":2,"10":3,"2":4,"b":5}"#).unwrap();
  let keys: Vec<&str> = tree.entries().iter().map(|(key, _)| key.as_str()).collect();

  assert_eq!(keys, ["2", "10", "b", "a"]);
  assert_eq!(tree.get("b").and_then(Tree::as_f64), Some(5.0));
}
