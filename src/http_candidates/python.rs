//! Deliberately bounded same-file Python profile, operating on syntax nodes.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;

type Mount = (String, Option<String>, Span);
#[derive(Clone)]
pub(super) struct Router {
    prefix: Option<String>,
    app: bool,
    span: Span,
}
pub(super) struct Profile<'a> {
    source: &'a str,
    clients: BTreeSet<String>,
    constructors: BTreeMap<String, bool>,
    routers: BTreeMap<String, Router>,
    handlers: BTreeMap<String, Owner>,
    mounts: BTreeMap<String, Vec<Mount>>,
    ambiguous: BTreeSet<String>,
}
fn text<'a>(n: Node, s: &'a str) -> &'a str {
    &s[n.byte_range()]
}
fn children(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.named_children(&mut c).collect()
}
fn descendants(n: Node) -> Vec<Node> {
    let mut out = vec![n];
    for c in children(n) {
        out.extend(descendants(c));
    }
    out
}
fn span(n: Node) -> Span {
    Span {
        start_byte: n.start_byte(),
        end_byte: n.end_byte(),
        start_line: n.start_position().row,
        end_line: n.end_position().row,
        start_column_byte: n.start_position().column,
        end_column_byte: n.end_position().column,
    }
}
/// Only plain, unescaped single-token Python strings are decoded in v1.
fn literal(n: Node, s: &str) -> Option<String> {
    if n.kind() != "string" {
        return None;
    }
    let raw = text(n, s);
    let q = raw.as_bytes().first().copied()?;
    if !b"'\"".contains(&q)
        || raw.len() < 2
        || raw.as_bytes().last() != Some(&q)
        || raw.contains('\\')
        || raw.starts_with("\"\"\"")
        || raw.starts_with("'''")
    {
        return None;
    }
    Some(raw[1..raw.len() - 1].into())
}
fn args(n: Node) -> Vec<Node> {
    n.child_by_field_name("arguments")
        .map(children)
        .unwrap_or_default()
}
fn keyword<'a>(n: Node<'a>, name: &str, s: &str) -> Option<Node<'a>> {
    args(n)
        .into_iter()
        .find(|a| {
            a.kind() == "keyword_argument"
                && a.child_by_field_name("name")
                    .is_some_and(|v| text(v, s) == name)
        })
        .and_then(|a| a.child_by_field_name("value"))
}
fn positional(n: Node, i: usize) -> Option<Node> {
    args(n)
        .into_iter()
        .filter(|a| a.kind() != "keyword_argument")
        .nth(i)
}
fn function<'a>(n: Node, s: &'a str) -> Option<&'a str> {
    Some(text(n.child_by_field_name("function")?, s))
}
fn owner(n: Node, s: &str) -> Owner {
    let mut scope = Vec::new();
    let mut p = n.parent();
    while let Some(a) = p {
        if ["function_definition", "class_definition"].contains(&a.kind()) {
            if let Some(name) = a.child_by_field_name("name") {
                scope.push(text(name, s).into());
            }
        }
        p = a.parent();
    }
    scope.reverse();
    Owner {
        name: n
            .child_by_field_name("name")
            .map_or("", |v| text(v, s))
            .into(),
        scope,
        span: span(n),
    }
}
fn caller(n: Node, s: &str) -> Option<Owner> {
    let mut p = n.parent();
    while let Some(a) = p {
        if a.kind() == "function_definition" {
            return Some(owner(a, s));
        }
        p = a.parent();
    }
    None
}
fn handler(n: Node, s: &str) -> Option<Owner> {
    let p = n.parent()?;
    if p.kind() == "decorator" {
        let d = p.parent()?;
        return children(d)
            .into_iter()
            .find(|c| c.kind() == "function_definition")
            .map(|f| owner(f, s));
    }
    None
}
fn method(s: &str) -> Option<String> {
    let v = s.to_ascii_uppercase();
    [
        "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT",
    ]
    .contains(&v.as_str())
    .then_some(v)
}
fn seed(n: Node, role: &str, s: &str) -> Candidate {
    Candidate {
        id: String::new(),
        role: role.into(),
        tier: "treesitter".into(),
        confidence: "candidate".into(),
        method: None,
        declared_path: None,
        path: None,
        authority: None,
        query_present: None,
        occurrence_span: span(n),
        prefix_spans: vec![],
        caller: caller(n, s),
        handler: None,
        resolution: "unresolved".into(),
        unresolved_reasons: vec![],
    }
}
fn reason(c: &mut Candidate, why: &str) {
    if !c.unresolved_reasons.iter().any(|r| r == why) {
        c.unresolved_reasons.push(why.into());
    }
}
impl Profile<'_> {
    fn prefixes(
        &self,
        name: &str,
        seen: &mut BTreeSet<String>,
    ) -> Vec<(Option<String>, Vec<Span>)> {
        if !seen.insert(name.into()) || self.ambiguous.contains(name) {
            return vec![(None, vec![])];
        }
        let Some(r) = self.routers.get(name) else {
            return vec![(None, vec![])];
        };
        if r.app {
            seen.remove(name);
            return vec![(r.prefix.clone(), vec![r.span.clone()])];
        }
        let mut result = vec![];
        if let Some(mounts) = self.mounts.get(name) {
            for (parent, prefix, location) in mounts {
                for (base, mut provenance) in self.prefixes(parent, seen) {
                    provenance.extend([location.clone(), r.span.clone()]);
                    result.push((
                        base.zip(prefix.clone())
                            .zip(r.prefix.clone())
                            .map(|((a, b), c)| a + &b + &c),
                        provenance,
                    ));
                }
            }
        }
        seen.remove(name);
        if result.is_empty() {
            result.push((None, vec![r.span.clone()]));
        }
        result
    }
    fn producer(&self, n: Node, receiver: &str, verb: &str) -> Vec<Candidate> {
        let mut base = seed(n, "producer", self.source);
        base.handler = handler(n, self.source);
        let path = positional(n, 0)
            .or_else(|| keyword(n, "path", self.source))
            .and_then(|v| literal(v, self.source));
        base.declared_path = path.clone();
        if path.as_ref().is_none_or(|v| !v.starts_with('/')) {
            reason(&mut base, "dynamic_path");
        }
        let methods = if verb == "add_api_route" {
            base.handler = keyword(n, "endpoint", self.source)
                .or_else(|| positional(n, 1))
                .and_then(|v| {
                    if v.kind() != "identifier" {
                        return None;
                    }
                    let name = text(v, self.source);
                    self.handlers
                        .get(name)
                        .filter(|h| {
                            !self.ambiguous.contains(name)
                                && caller(n, self.source).is_none()
                                && h.span.end_byte <= n.start_byte()
                        })
                        .cloned()
                });
            keyword(n, "methods", self.source)
                .filter(|a| ["list", "tuple"].contains(&a.kind()))
                .map(|a| {
                    children(a)
                        .into_iter()
                        .map(|m| literal(m, self.source).and_then(|v| method(&v)))
                        .collect::<Vec<_>>()
                })
                .filter(|m| !m.is_empty())
                .unwrap_or(vec![None])
        } else {
            vec![method(verb)]
        };
        if base.handler.is_none() {
            reason(&mut base, "unknown_handler");
        }
        let mut out = vec![];
        let methods: BTreeSet<_> = methods.into_iter().collect();
        for method in methods {
            for (prefix, provenance) in self.prefixes(receiver, &mut BTreeSet::new()) {
                let mut c = base.clone();
                c.method = method.clone();
                c.prefix_spans = provenance;
                if c.method.is_none() {
                    reason(&mut c, "dynamic_method");
                }
                if prefix.is_none() {
                    reason(&mut c, "unknown_router_prefix");
                }
                c.path = prefix.zip(path.clone()).map(|(a, b)| a + &b);
                if c.unresolved_reasons.is_empty() {
                    c.resolution = "literal".into();
                }
                out.push(c);
            }
        }
        out
    }
    fn consumer(&self, n: Node, receiver: &str, verb: &str) -> Candidate {
        let mut c = seed(n, "consumer", self.source);
        c.method = method(verb);
        if self.ambiguous.contains(receiver) {
            reason(&mut c, "ambiguous_binding");
        }
        let raw = positional(n, 0)
            .or_else(|| keyword(n, "url", self.source))
            .and_then(|v| literal(v, self.source));
        match raw.and_then(|s| url::Url::parse(&s).ok().map(|u| (s, u))) {
            Some((raw, u))
                if ["http", "https"].contains(&u.scheme())
                    && u.host_str().is_some()
                    && raw
                        .split_once("://")
                        .is_some_and(|(_, s)| !s.is_empty() && !s.starts_with(['/', '?', '#']))
                    && !raw.contains('\\')
                    && !raw.chars().any(char::is_whitespace) =>
            {
                if !u.username().is_empty() || u.password().is_some() {
                    reason(&mut c, "credentialled_url");
                } else {
                    // url::Url normalizes dot segments; preserve the literal escaped request path.
                    let suffix = raw.split_once("://").map_or("", |(_, s)| s);
                    let tail = suffix.find(['/', '?', '#']).map_or("", |i| &suffix[i..]);
                    let path = tail
                        .split(['?', '#'])
                        .next()
                        .filter(|p| !p.is_empty())
                        .unwrap_or("/");
                    c.path = Some(path.into());
                    c.declared_path = c.path.clone();
                    c.query_present = Some(u.query().is_some());
                    c.authority = Some(Authority {
                        scheme: u.scheme().into(),
                        host: u.host_str().unwrap_or("").into(),
                        port: {
                            let authority = suffix.split(['/', '?', '#']).next().unwrap_or("");
                            authority
                                .rsplit_once(':')
                                .and_then(|(_, p)| p.parse::<u16>().ok())
                        },
                    });
                    if keyword(n, "params", self.source).is_some() {
                        c.query_present = Some(true);
                    }
                    if keyword(n, "base_url", self.source).is_some() {
                        reason(&mut c, "unknown_base_url");
                    }
                }
            }
            _ => reason(&mut c, "unknown_base_url"),
        }
        if c.unresolved_reasons.is_empty() {
            c.resolution = "literal".into();
        }
        c
    }
}
mod scan;
pub(super) use scan::extract;
