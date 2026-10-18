//! Binding collection and candidate traversal.
use super::*;
pub(in crate::http_candidates) fn extract(source: &str, module: &str) -> Result<FileCandidates> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| Error::Parse(e.to_string()))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| Error::Parse("Python parser unavailable".into()))?;
    let root = tree.root_node();
    let mut file = FileCandidates {
        module: module.into(),
        source_sha256: hex::encode(Sha256::digest(source.as_bytes())),
        language: "python".into(),
        snapshot_state: "working_tree".into(),
        candidates: vec![],
        diagnostics: vec![],
    };
    if root.has_error() {
        file.diagnostics
            .push("parse_error: no candidates admitted".into());
        return Ok(file);
    }
    let mut p = Profile {
        source,
        clients: BTreeSet::new(),
        constructors: BTreeMap::new(),
        routers: BTreeMap::new(),
        handlers: BTreeMap::new(),
        mounts: BTreeMap::new(),
        ambiguous: BTreeSet::new(),
    };
    // Imports and router declarations must be module-level, with explicit bindings.
    for stmt in children(root) {
        // Explicit registration only resolves stable, undecorated module functions.
        if stmt.kind() == "function_definition" {
            let h = owner(stmt, source);
            if p.handlers.insert(h.name.clone(), h.clone()).is_some() {
                p.ambiguous.insert(h.name);
            }
        }
        if stmt.kind() == "import_statement" {
            for item in children(stmt) {
                let (name, alias) = if item.kind() == "aliased_import" {
                    (
                        item.child_by_field_name("name"),
                        item.child_by_field_name("alias"),
                    )
                } else {
                    (Some(item), Some(item))
                };
                if let (Some(name), Some(alias)) = (name, alias) {
                    if ["requests", "httpx"].contains(&text(name, source)) {
                        p.clients.insert(text(alias, source).into());
                    }
                }
            }
        }
        if stmt.kind() == "import_from_statement"
            && stmt
                .child_by_field_name("module_name")
                .is_some_and(|m| text(m, source) == "fastapi")
        {
            for item in children(stmt) {
                let (name, alias) = if item.kind() == "aliased_import" {
                    (
                        item.child_by_field_name("name"),
                        item.child_by_field_name("alias"),
                    )
                } else {
                    (Some(item), Some(item))
                };
                if let (Some(n), Some(a)) = (name, alias) {
                    match text(n, source) {
                        "FastAPI" => {
                            p.constructors.insert(text(a, source).into(), true);
                        }
                        "APIRouter" => {
                            p.constructors.insert(text(a, source).into(), false);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    for stmt in children(root) {
        for n in children(stmt) {
            if n.kind() != "assignment" {
                continue;
            }
            let (Some(left), Some(right)) = (
                n.child_by_field_name("left"),
                n.child_by_field_name("right"),
            ) else {
                continue;
            };
            if left.kind() != "identifier" || right.kind() != "call" {
                continue;
            }
            let Some(app) = function(right, source)
                .and_then(|f| p.constructors.get(f))
                .copied()
            else {
                continue;
            };
            let prefix = keyword(right, "prefix", source)
                .map_or(Some(String::new()), |v| literal(v, source));
            let prefix = if app && keyword(right, "prefix", source).is_some() {
                None
            } else {
                prefix
            };
            let prefix =
                prefix.filter(|v| v.is_empty() || (v.starts_with('/') && !v.ends_with('/')));
            let name = text(left, source).to_string();
            if p.routers
                .insert(
                    name.clone(),
                    Router {
                        prefix,
                        app,
                        span: span(n),
                    },
                )
                .is_some()
            {
                p.ambiguous.insert(name);
            }
        }
    }
    let all = descendants(root);
    // Any rebinding, including nested function parameters, invalidates that alias
    // conservatively for the whole file. No interprocedural execution is inferred.
    for n in &all {
        if n.kind() == "identifier" {
            let name = text(*n, source);
            if !p.clients.contains(name)
                && !p.routers.contains_key(name)
                && !p.constructors.contains_key(name)
                && !p.handlers.contains_key(name)
            {
                continue;
            }
            let Some(parent) = n.parent() else {
                continue;
            };
            let binding = parent.child_by_field_name("left") == Some(*n)
                || (parent.child_by_field_name("name") == Some(*n)
                    && ["function_definition", "class_definition"].contains(&parent.kind()))
                || [
                    "parameters",
                    "lambda_parameters",
                    "list_splat_pattern",
                    "dictionary_splat_pattern",
                    "as_pattern_target",
                    "default_parameter",
                    "typed_default_parameter",
                    "typed_parameter",
                    "for_statement",
                    "as_pattern",
                ]
                .contains(&parent.kind());
            let recognized = (parent.kind() == "assignment"
                && p.routers.get(name).is_some_and(|r| r.span == span(parent)))
                || (parent.kind() == "function_definition"
                    && !p.clients.contains(name)
                    && !p.routers.contains_key(name)
                    && !p.constructors.contains_key(name)
                    && p.handlers.get(name).is_some_and(|h| h.span == span(parent)));
            let mut ancestor = Some(parent);
            let mut nested_binding = false;
            while let Some(a) = ancestor {
                if [
                    "assignment",
                    "augmented_assignment",
                    "for_statement",
                    "for_in_clause",
                    "named_expression",
                ]
                .contains(&a.kind())
                    && a.child_by_field_name("left")
                        .or_else(|| a.child_by_field_name("name"))
                        .is_some_and(|l| {
                            l.start_byte() <= n.start_byte() && l.end_byte() >= n.end_byte()
                        })
                {
                    nested_binding = true;
                }
                if a.kind() == "aliased_import" && a.child_by_field_name("alias") == Some(*n) {
                    let imported = a
                        .child_by_field_name("name")
                        .map_or("", |v| text(v, source));
                    let valid = (["requests", "httpx"].contains(&imported)
                        && p.clients.contains(name))
                        || (["FastAPI", "APIRouter"].contains(&imported)
                            && p.constructors.contains_key(name));
                    if !valid {
                        nested_binding = true;
                    }
                }
                if a.kind() == "import_from_statement" {
                    let is_fastapi = a
                        .child_by_field_name("module_name")
                        .is_some_and(|m| text(m, source) == "fastapi");
                    for item in children(a) {
                        if Some(item) == a.child_by_field_name("module_name")
                            || item.start_byte() > n.start_byte()
                            || item.end_byte() < n.end_byte()
                        {
                            continue;
                        }
                        let imported = item.child_by_field_name("name").unwrap_or(item);
                        if !(is_fastapi
                            && p.constructors.contains_key(name)
                            && ["FastAPI", "APIRouter"].contains(&text(imported, source)))
                        {
                            nested_binding = true;
                        }
                    }
                }
                if ["as_pattern_target", "case_pattern"].contains(&a.kind()) {
                    nested_binding = true;
                }
                if a.kind() == "import_statement" {
                    for item in children(a) {
                        let imported = item.child_by_field_name("name").unwrap_or(item);
                        let bound = item.child_by_field_name("alias").map_or_else(
                            || text(imported, source).split('.').next().unwrap_or(""),
                            |v| text(v, source),
                        );
                        let valid = p.clients.contains(name)
                            && ["requests", "httpx"].contains(&text(imported, source));
                        if bound == name && !valid {
                            nested_binding = true;
                        }
                    }
                }
                if a.kind() == "delete_statement" {
                    nested_binding = true;
                }
                ancestor = a.parent();
            }
            if (binding || nested_binding) && !recognized {
                p.ambiguous.insert(name.into());
            }
        }
    }
    if all.iter().any(|n| n.kind() == "wildcard_import") {
        p.ambiguous.extend(p.clients.iter().cloned());
        p.ambiguous.extend(p.constructors.keys().cloned());
        p.ambiguous.extend(p.routers.keys().cloned());
        p.ambiguous.extend(p.handlers.keys().cloned());
    }
    if p.constructors.keys().any(|name| p.ambiguous.contains(name)) {
        p.ambiguous.extend(p.routers.keys().cloned());
    }
    for n in &all {
        if n.kind() != "call" {
            continue;
        }
        let Some(f) = function(*n, source) else {
            continue;
        };
        let Some((parent, verb)) = f.rsplit_once('.') else {
            continue;
        };
        if verb != "include_router" || !p.routers.contains_key(parent) {
            continue;
        }
        let Some(child) = positional(*n, 0)
            .filter(|c| c.kind() == "identifier")
            .map(|c| text(c, source))
        else {
            continue;
        };
        if caller(*n, source).is_some() {
            p.ambiguous.insert(child.into());
            continue;
        }
        let prefix =
            keyword(*n, "prefix", source).map_or(Some(String::new()), |v| literal(v, source));
        let prefix = prefix.filter(|v| v.is_empty() || (v.starts_with('/') && !v.ends_with('/')));
        p.mounts
            .entry(child.into())
            .or_default()
            .push((parent.into(), prefix, span(*n)));
    }
    for n in all {
        if n.kind() != "call" {
            continue;
        }
        let Some(f) = function(n, source) else {
            continue;
        };
        let Some((receiver, verb)) = f.rsplit_once('.') else {
            continue;
        };
        if p.routers.contains_key(receiver) && (method(verb).is_some() || verb == "add_api_route") {
            // HTTP method attributes are routes only in decorators; registration is explicit.
            if verb == "add_api_route" || n.parent().is_some_and(|a| a.kind() == "decorator") {
                file.candidates.extend(p.producer(n, receiver, verb));
            }
        } else if p.clients.contains(receiver) && method(verb).is_some() {
            file.candidates.push(p.consumer(n, receiver, verb));
        }
    }
    let generated = source.lines().take(3).any(|l| l.trim() == "# @generated");
    for c in &mut file.candidates {
        if generated {
            reason(c, "generated_client");
            c.resolution = "unresolved".into();
        }

        c.id = candidate_id("", None, &file.module, &file.source_sha256, c)?;
    }
    Ok(file)
}
