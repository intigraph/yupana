//! Parse launchd scheduling values, retaining array multiplicity for shrink-only
//! admission. XML comments/formatting do not become scheduler definitions.

use anyhow::{bail, ensure, Context, Result};
use quick_xml::events::Event;

use super::Fact;

#[derive(Debug, Default)]
struct Node {
    tag: String,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn canonical(&self) -> Result<String> {
        if self.tag == "dict" {
            let mut fields = pairs(self)?
                .into_iter()
                .map(|(key, value)| Ok(format!("{key}={}", value.canonical()?)))
                .collect::<Result<Vec<_>>>()?;
            fields.sort();
            Ok(format!("dict({})", fields.join(",")))
        } else {
            Ok(format!(
                "{}({}{})",
                self.tag,
                self.text.trim(),
                self.children
                    .iter()
                    .map(Node::canonical)
                    .collect::<Result<Vec<_>>>()?
                    .join(",")
            ))
        }
    }
}

fn pairs(node: &Node) -> Result<Vec<(&str, &Node)>> {
    ensure!(
        node.children.len().is_multiple_of(2),
        "plist dictionary has no paired value"
    );
    let mut out = Vec::new();
    let mut keys = std::collections::BTreeSet::new();
    for pair in node.children.as_chunks::<2>().0 {
        ensure!(pair[0].tag == "key", "plist dictionary lacks key");
        let key = pair[0].text.trim();
        ensure!(keys.insert(key), "duplicate plist dictionary key");
        out.push((key, &pair[1]));
    }
    Ok(out)
}

pub(super) fn facts(path: &str, text: &str) -> Result<Vec<Fact>> {
    let mut reader = quick_xml::Reader::from_str(text);
    let mut stack = vec![Node::default()];
    loop {
        match reader.read_event().context("malformed plist XML")? {
            Event::Start(e) => {
                ensure!(stack.len() < 128, "plist nesting exceeds bound");
                stack.push(Node {
                    tag: String::from_utf8(e.name().as_ref().to_vec())?,
                    ..Node::default()
                });
            }
            Event::Empty(e) => stack
                .last_mut()
                .context("missing plist parent")?
                .children
                .push(Node {
                    tag: String::from_utf8(e.name().as_ref().to_vec())?,
                    ..Node::default()
                }),
            Event::Text(e) => stack
                .last_mut()
                .context("missing plist parent")?
                .text
                .push_str(&quick_xml::escape::unescape(&e.decode()?)?),
            Event::End(e) => {
                ensure!(stack.len() > 1, "unbalanced plist XML");
                let node = stack.pop().context("missing plist node")?;
                ensure!(
                    node.tag.as_bytes() == e.name().as_ref(),
                    "unbalanced plist tag"
                );
                stack
                    .last_mut()
                    .context("missing plist parent")?
                    .children
                    .push(node);
            }
            Event::GeneralRef(_) | Event::CData(_) => bail!("unsupported plist entity/CDATA"),
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(stack.len() == 1, "incomplete plist XML");
    let document = stack.pop().context("missing plist document")?;
    ensure!(
        document.children.len() == 1 && document.children[0].tag == "plist",
        "not a plist document"
    );
    let root = &document.children[0];
    ensure!(
        root.children.len() == 1 && root.children[0].tag == "dict",
        "launchd plist root must be dictionary"
    );
    let fields = pairs(&root.children[0])?;
    let context = fields
        .iter()
        .filter(|(key, _)| matches!(*key, "Program" | "ProgramArguments"))
        .map(|(key, value)| Ok(format!("{key}={}", value.canonical()?)))
        .collect::<Result<Vec<_>>>()?
        .join(",");
    let mut out = Vec::new();
    for (key, value) in fields {
        if !matches!(key, "StartInterval" | "StartCalendarInterval") {
            continue;
        }
        let entries = if value.tag == "array" {
            value.children.iter().collect::<Vec<_>>()
        } else {
            vec![value]
        };
        for entry in entries {
            out.push(Fact::new(
                path,
                "launchd",
                format!("{key}={} {context}", entry.canonical()?),
                true,
            ));
        }
    }
    Ok(out)
}
