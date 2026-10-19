use oxc_ast::ast::Statement;

use bmo_explorer::site::readers::script::*;

#[test]
fn a_json_value_is_read_where_it_stands_and_no_further() {
  let source = "window.__STATE__ = {\"a\": [1, 2], \"b\": \"x\"};\nrun();";

  assert_eq!(
    json_after(source, "window.__STATE__ = "),
    Some(serde_json::json!({ "a": [1, 2], "b": "x" }))
  );
  assert_eq!(json_at(source, 0), None);
}

#[test]
fn a_script_s_literal_is_read_as_the_value_it_writes() {
  let read = with_program(
    "var data = { name: 'a', list: [1, -2, true, null, `t`], 'quoted key': { n: 1.5 } };",
    |program| {
      let Statement::VariableDeclaration(declaration) = &program.body[0] else { return None };

      literal_value(declaration.declarations[0].init.as_ref()?)
    },
  );

  assert_eq!(
    read.flatten(),
    Some(serde_json::json!({ "name": "a", "list": [1, -2, true, null, "t"], "quoted key": { "n": 1.5 } }))
  );
  assert_eq!(with_program("var a = f();", |program| program.body.len()), Some(1));
}
