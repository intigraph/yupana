//! Conservative lexical binding hints, not Python execution/type inference.
use std::collections::HashMap;
use tree_sitter::Node;

use super::CallBinding;

#[derive(Default)]
struct Scope {
    parent: Option<usize>,
    bindings: HashMap<String, Vec<CallBinding>>,
    wildcard: bool,
    class_namespace: bool,
}

fn text(node: Node<'_>, bytes: &[u8]) -> String {
    node.utf8_text(bytes).unwrap_or_default().to_owned()
}

fn bind(scope: &mut Scope, name: String, binding: CallBinding) {
    let entries = scope.bindings.entry(name).or_default();
    if !entries.contains(&binding) {
        entries.push(binding);
    }
}

/// One linear AST pass records lexical bindings and call scopes. A second pass
/// resolves names through those scopes; duplicate/rebound imports are unknown.
pub(super) fn hints(root: Node<'_>, bytes: &[u8]) -> HashMap<usize, CallBinding> {
    let mut scopes = vec![Scope::default()];
    let mut calls = Vec::new();
    let mut stack = vec![(root, 0)];
    while let Some((node, mut scope)) = stack.pop() {
        match node.kind() {
            "function_definition" | "class_definition" | "lambda" => {
                if let Some(name) = node.child_by_field_name("name") {
                    bind(&mut scopes[scope], text(name, bytes), CallBinding::Local);
                }
                // A class namespace is not a lexical closure for methods or
                // lambdas, including decorated methods. Preserve any enclosing
                // FUNCTION scope when skipping it (nested classes can close over it).
                let parent = if node.kind() != "class_definition" && scopes[scope].class_namespace {
                    scopes[scope].parent
                } else {
                    Some(scope)
                };
                let id = scopes.len();
                scopes.push(Scope {
                    parent,
                    class_namespace: node.kind() == "class_definition",
                    ..Scope::default()
                });
                scope = id;
                if let Some(params) = node.child_by_field_name("parameters") {
                    // Conservative for typed/default parameters: extra identifiers
                    // only suppress an inference; they never establish a target.
                    let mut pending = vec![params];
                    while let Some(param) = pending.pop() {
                        if param.kind() == "identifier" {
                            bind(&mut scopes[scope], text(param, bytes), CallBinding::Unknown);
                        }
                        let mut cursor = param.walk();
                        pending.extend(param.named_children(&mut cursor));
                    }
                }
            }
            "import_statement" | "import_from_statement" => {
                let module = node
                    .child_by_field_name("module_name")
                    .map(|n| text(n, bytes));
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if Some(child) == node.child_by_field_name("module_name") {
                        continue;
                    }
                    if child.kind() == "wildcard_import" {
                        scopes[scope].wildcard = true;
                        continue;
                    }
                    let (name, alias) = if child.kind() == "aliased_import" {
                        let Some(name) = child.child_by_field_name("name") else {
                            continue;
                        };
                        (
                            text(name, bytes),
                            child.child_by_field_name("alias").map(|n| text(n, bytes)),
                        )
                    } else if child.kind() == "dotted_name" {
                        (text(child, bytes), None)
                    } else {
                        continue;
                    };
                    let (local, binding) = if let Some(module) = &module {
                        (
                            alias.unwrap_or_else(|| name.clone()),
                            CallBinding::Imported {
                                module: module.clone(),
                                name,
                            },
                        )
                    } else {
                        let local = alias
                            .clone()
                            .unwrap_or_else(|| name.split('.').next().unwrap_or(&name).to_owned());
                        let module = if alias.is_some() { name } else { local.clone() };
                        (local, CallBinding::Module(module))
                    };
                    bind(&mut scopes[scope], local, binding);
                }
                // Import identifiers must not be interpreted as assignments.
                continue;
            }
            "assignment" | "augmented_assignment" | "named_expression" | "for_statement" => {
                if let Some(left) = node
                    .child_by_field_name("left")
                    .or_else(|| node.child_by_field_name("name"))
                {
                    let mut pending = vec![left];
                    while let Some(n) = pending.pop() {
                        if n.kind() == "identifier" {
                            bind(&mut scopes[scope], text(n, bytes), CallBinding::Unknown);
                        }
                        if n.kind() == "attribute" || n.kind() == "subscript" {
                            continue;
                        }
                        let mut cursor = n.walk();
                        pending.extend(n.named_children(&mut cursor));
                    }
                }
            }
            "except_clause"
            | "with_item"
            | "list_comprehension"
            | "set_comprehension"
            | "dictionary_comprehension"
            | "generator_expression"
            | "match_statement"
            | "delete_statement" => {
                scopes[scope].wildcard = true;
            }
            "global_statement" | "nonlocal_statement" => {
                let mut cursor = node.walk();
                for name in node.named_children(&mut cursor) {
                    bind(&mut scopes[scope], text(name, bytes), CallBinding::Unknown);
                }
            }
            "call" => calls.push((node, scope)),
            _ => {}
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor).map(|child| (child, scope)));
    }
    calls
        .into_iter()
        .map(|(node, scope)| {
            let function = node.child_by_field_name("function");
            let expression = function.map(|n| text(n, bytes)).unwrap_or_default();
            let mut parts = expression.split('.');
            let first = parts.next().unwrap_or_default();
            let rest: Vec<_> = parts.collect();
            let mut current = Some(scope);
            let mut binding = CallBinding::Unqualified;
            while let Some(id) = current {
                if scopes[id].wildcard {
                    binding = CallBinding::Unknown;
                    break;
                }
                if let Some(entries) = scopes[id].bindings.get(first) {
                    binding = if entries.len() == 1 {
                        entries[0].clone()
                    } else {
                        CallBinding::Unknown
                    };
                    break;
                }
                if scopes[id].wildcard {
                    binding = CallBinding::Unknown;
                    break;
                }
                current = scopes[id].parent;
            }
            let binding = if rest.is_empty() {
                match binding {
                    CallBinding::Module(_) => CallBinding::Unknown,
                    other => other,
                }
            } else {
                match binding {
                    CallBinding::Module(module) if rest.len() == 1 => CallBinding::Imported {
                        module,
                        name: rest[0].to_owned(),
                    },
                    _ => CallBinding::Unknown,
                }
            };
            (node.id(), binding)
        })
        .collect()
}
