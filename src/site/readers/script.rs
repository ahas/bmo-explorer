//! Data a page's own scripts carry, read as data and never run: a JSON value standing at a place in a script,
//! read by the JSON reader up to where the value ends; and a script read whole by a JavaScript parser (oxc),
//! its literals — objects, arrays, strings, numbers, booleans and null, keys quoted or not — taken as the
//! values they write.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
  ArrayExpressionElement, Expression, ObjectExpression, ObjectPropertyKind, Program, PropertyKey,
  UnaryOperator,
};
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde_json::{Map, Number, Value};

use crate::site::readers::ordered::Tree;

/// The JSON value standing at `at` in a source, its leading whitespace passed over, read up to where the value
/// ends and no further; none where what stands there is no JSON value.
pub fn json_at(source: &str, at: usize) -> Option<Value> {
  let rest = source.get(at..)?;

  serde_json::Deserializer::from_str(rest).into_iter::<Value>().next()?.ok()
}

/// The JSON value standing just after the first place `opening` is written in the source.
pub fn json_after(source: &str, opening: &str) -> Option<Value> {
  source.match_indices(opening).find_map(|(at, _)| json_at(source, at + opening.len()))
}

/// A script read whole, handed to `read`; none where it does not parse.
pub fn with_program<T>(source: &str, read: impl FnOnce(&Program<'_>) -> T) -> Option<T> {
  let allocator = Allocator::default();
  let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();

  if parsed.fatal_error {
    return None;
  }

  Some(read(&parsed.program))
}

/// One expression read whole, handed to `read`; none where the source is no single expression.
pub fn with_expression<T>(source: &str, read: impl FnOnce(&Expression<'_>) -> T) -> Option<T> {
  let allocator = Allocator::default();
  let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse_expression().ok()?;

  Some(read(&parsed))
}

/// The name a property is keyed by, where it is spelled out: an identifier, a string or a number.
pub fn key_name(key: &PropertyKey<'_>) -> Option<String> {
  key.static_name().map(|name| name.to_string())
}

/// An object literal's property by its name.
pub fn property<'a, 'b>(object: &'b ObjectExpression<'a>, name: &str) -> Option<&'b Expression<'a>> {
  object.properties.iter().find_map(|one| match one {
    ObjectPropertyKind::ObjectProperty(property) if key_name(&property.key).as_deref() == Some(name) => {
      Some(&property.value)
    }
    _ => None,
  })
}

fn number(value: f64) -> Value {
  if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
    Value::Number(Number::from(value as i64))
  } else {
    Number::from_f64(value).map_or(Value::Null, Value::Number)
  }
}

/// The plain value a literal writes: objects and arrays of numbers, strings, booleans and null, a negative
/// number, and a template with nothing put into it. None for anything a script would have to run.
pub fn literal_value(expression: &Expression<'_>) -> Option<Value> {
  match expression {
    Expression::StringLiteral(literal) => Some(Value::String(literal.value.to_string())),
    Expression::NumericLiteral(literal) => Some(number(literal.value)),
    Expression::BooleanLiteral(literal) => Some(Value::Bool(literal.value)),
    Expression::NullLiteral(_) => Some(Value::Null),
    Expression::TemplateLiteral(template) if template.expressions.is_empty() => Some(Value::String(
      template
        .quasis
        .iter()
        .map(|quasi| quasi.value.cooked.as_deref().unwrap_or(quasi.value.raw.as_str()))
        .collect(),
    )),
    Expression::UnaryExpression(unary) if unary.operator == UnaryOperator::UnaryNegation => {
      match literal_value(&unary.argument)? {
        Value::Number(value) => Some(number(-value.as_f64()?)),
        _ => None,
      }
    }
    Expression::ParenthesizedExpression(inner) => literal_value(&inner.expression),
    Expression::ArrayExpression(array) => {
      let mut values = Vec::new();

      for element in &array.elements {
        match element {
          ArrayExpressionElement::SpreadElement(_) => return None,
          ArrayExpressionElement::Elision(_) => values.push(Value::Null),
          other => values.push(literal_value(other.as_expression()?)?),
        }
      }

      Some(Value::Array(values))
    }
    Expression::ObjectExpression(object) => {
      let mut values = Map::new();

      for one in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = one else { return None };
        let name = key_name(&property.key)?;

        values.insert(name, literal_value(&property.value)?);
      }

      Some(Value::Object(values))
    }
    _ => None,
  }
}

/// The plain value a literal writes, as `literal_value` reads it, its objects' fields kept in the order the script
/// walks them.
pub fn literal_tree(expression: &Expression<'_>) -> Option<Tree> {
  match expression {
    Expression::ParenthesizedExpression(inner) => literal_tree(&inner.expression),
    Expression::ArrayExpression(array) => {
      let mut items = Vec::new();

      for element in &array.elements {
        match element {
          ArrayExpressionElement::SpreadElement(_) => return None,
          ArrayExpressionElement::Elision(_) => items.push(Tree::Scalar(Value::Null)),
          other => items.push(literal_tree(other.as_expression()?)?),
        }
      }

      Some(Tree::List(items))
    }
    Expression::ObjectExpression(object) => {
      let mut fields = Vec::new();

      for one in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = one else { return None };

        fields.push((key_name(&property.key)?, literal_tree(&property.value)?));
      }

      Some(Tree::fields(fields))
    }
    other => literal_value(other).map(Tree::Scalar),
  }
}

/// The values a literal holds where the script would work some of them out: an object keeps the properties it
/// spells out as values, and every value the script would have to run — the element a page is drawn into — is
/// read as null.
pub fn loose_value(expression: &Expression<'_>) -> Value {
  match expression {
    Expression::ObjectExpression(object) => Value::Object(
      object
        .properties
        .iter()
        .filter_map(|one| match one {
          ObjectPropertyKind::ObjectProperty(property) => {
            key_name(&property.key).map(|name| (name, loose_value(&property.value)))
          }
          ObjectPropertyKind::SpreadProperty(_) => None,
        })
        .collect(),
    ),
    Expression::ArrayExpression(array) => Value::Array(
      array
        .elements
        .iter()
        .map(|element| element.as_expression().map_or(Value::Null, loose_value))
        .collect(),
    ),
    other => literal_value(other).unwrap_or(Value::Null),
  }
}

/// The arguments of the call — or the construction with `new` — written at `at` in a source, each read as
/// `loose_value` reads it; none where no call stands there. The call is read up to the first closing
/// parenthesis at which the parser takes everything from `at` as one call, which is where the call ends.
pub fn call_arguments_at(source: &str, at: usize) -> Option<Vec<Value>> {
  let rest = source.get(at..)?;

  rest.match_indices(')').find_map(|(end, _)| {
    with_expression(&rest[..=end], |expression| {
      let arguments = match expression {
        Expression::CallExpression(call) => &call.arguments,
        Expression::NewExpression(construction) => &construction.arguments,
        _ => return None,
      };

      Some(
        arguments
          .iter()
          .map(|argument| argument.as_expression().map_or(Value::Null, loose_value))
          .collect(),
      )
    })
    .flatten()
  })
}

/// The value a script declares a variable with, where it writes it as a literal: `var BCData = {…}`. The whole
/// script is read by the parser and nothing of it is run.
pub fn declared_value(source: &str, name: &str) -> Option<Value> {
  struct Declared<'n> {
    name: &'n str,
    found: Option<Value>,
  }

  impl<'a> oxc_ast_visit::Visit<'a> for Declared<'_> {
    fn visit_variable_declarator(&mut self, it: &oxc_ast::ast::VariableDeclarator<'a>) {
      if self.found.is_none()
        && let (oxc_ast::ast::BindingPattern::BindingIdentifier(id), Some(init)) = (&it.id, &it.init)
        && id.name == self.name
      {
        self.found = literal_value(init);
      }

      oxc_ast_visit::walk::walk_variable_declarator(self, it);
    }
  }

  with_program(source, |program| {
    let mut declared = Declared { name, found: None };

    oxc_ast_visit::Visit::visit_program(&mut declared, program);
    declared.found
  })
  .flatten()
}

/// Every call a script makes to a function by its name — `name(…)`, or a method `….name(…)` — in the order they
/// are written, each call's arguments read as `loose_value` reads them. The whole script is read by the parser and
/// nothing of it is run.
pub fn calls_named(source: &str, name: &str) -> Vec<Vec<Value>> {
  struct Calls<'n> {
    name: &'n str,
    found: Vec<Vec<Value>>,
  }

  impl<'a> oxc_ast_visit::Visit<'a> for Calls<'_> {
    fn visit_call_expression(&mut self, it: &oxc_ast::ast::CallExpression<'a>) {
      let named = match &it.callee {
        Expression::Identifier(identifier) => identifier.name == self.name,
        other => {
          other.as_member_expression().and_then(|member| member.static_property_name()) == Some(self.name)
        }
      };

      if named {
        self.found.push(
          it.arguments
            .iter()
            .map(|argument| argument.as_expression().map_or(Value::Null, loose_value))
            .collect(),
        );
      }

      oxc_ast_visit::walk::walk_call_expression(self, it);
    }
  }

  with_program(source, |program| {
    let mut calls = Calls {
      name,
      found: Vec::new(),
    };

    oxc_ast_visit::Visit::visit_program(&mut calls, program);
    calls.found
  })
  .unwrap_or_default()
}

/// The value the first object property keyed `name` holds anywhere in a script, where it writes it as a literal:
/// a setting of `var oSetting = { name: "…" }`. The whole script is read by the parser and nothing of it is run.
pub fn property_named(source: &str, name: &str) -> Option<Value> {
  struct Properties<'n> {
    name: &'n str,
    found: Option<Value>,
  }

  impl<'a> oxc_ast_visit::Visit<'a> for Properties<'_> {
    fn visit_object_property(&mut self, it: &oxc_ast::ast::ObjectProperty<'a>) {
      if self.found.is_none() && key_name(&it.key).as_deref() == Some(self.name) {
        self.found = literal_value(&it.value);
      }

      oxc_ast_visit::walk::walk_object_property(self, it);
    }
  }

  with_program(source, |program| {
    let mut properties = Properties { name, found: None };

    oxc_ast_visit::Visit::visit_program(&mut properties, program);
    properties.found
  })
  .flatten()
}

/// The span an expression covers in its source, as byte offsets.
pub fn span_of(expression: &Expression<'_>) -> (usize, usize) {
  use oxc_span::GetSpan;

  let span = expression.span();

  (span.start as usize, span.end as usize)
}
