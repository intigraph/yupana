//! Candidate filtering shared by the base and composed tenant graph.
use crate::extract::{CallBinding, CallSite};

/// The name referenced by a call, before graph lookup (aliases retain their target).
pub(super) fn target_name(call: &CallSite) -> &str {
    match &call.binding {
        CallBinding::Imported { name, .. } => name,
        _ => &call.callee,
    }
}

/// Conservative module identity. No filesystem imports, environment search paths,
/// re-exports or dynamic execution. Multiple matching roots remain ambiguous.
pub(super) fn accepts(
    call: &CallSite,
    caller_file: &str,
    target_file: &str,
    target_scope: &[String],
) -> bool {
    match &call.binding {
        CallBinding::Local => target_file == caller_file,
        CallBinding::Imported { module, .. } => {
            if !target_scope.is_empty() {
                return false;
            }
            let dots = module.chars().take_while(|c| *c == '.').count();
            let tail = module[dots..].replace('.', "/");
            let path = if dots == 0 {
                tail
            } else {
                let mut parent = std::path::Path::new(caller_file)
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(""));
                for _ in 1..dots {
                    let Some(up) = parent.parent() else {
                        return false;
                    };
                    parent = up;
                }
                if tail.is_empty() {
                    parent.to_string_lossy().into_owned()
                } else {
                    parent.join(tail).to_string_lossy().into_owned()
                }
            };
            target_file == format!("{path}.py") || target_file == format!("{path}/__init__.py")
        }
        _ => true,
    }
}

/// A call's evidence and uncertainty. A single candidate is still an AST hint,
/// never semantic/type-checker proof. Dynamic calls retain conservative edges.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "mcp", derive(schemars::JsonSchema))]
pub struct CallResolution {
    /// Source file.
    pub file: String,
    /// Enclosing caller.
    pub caller: String,
    /// Call line.
    pub line: usize,
    /// Name as written (possibly an alias).
    pub callee: String,
    /// `import_candidate`, `local_candidate`, `name_candidate`, `ambiguous`,
    /// `unresolved`, or `unsupported`.
    pub status: String,
    /// Number of surviving definitions; not coverage or precision.
    pub candidate_count: usize,
}

pub(super) fn report(file: &str, call: &CallSite, candidate_count: usize) -> CallResolution {
    let status = match (&call.binding, candidate_count) {
        (CallBinding::Unknown | CallBinding::Module(_), _) => "unsupported",
        (_, 0) => "unresolved",
        (_, 2..) => "ambiguous",
        (CallBinding::Imported { .. }, _) => "import_candidate",
        (CallBinding::Local, _) => "local_candidate",
        _ => "name_candidate",
    };
    CallResolution {
        file: file.to_owned(),
        caller: call.caller.clone(),
        line: call.line,
        callee: call.callee.clone(),
        status: status.to_owned(),
        candidate_count,
    }
}

impl super::CodeGraph {
    /// Per-call AST evidence for the named caller. Uncertainty is never a zero
    /// precision claim; unsupported calls retain the old conservative candidates.
    #[must_use]
    pub fn call_resolution(&self, caller: &str) -> Vec<CallResolution> {
        let mut out = Vec::new();
        for &from in self.by_name.get(caller).map_or(&[][..], Vec::as_slice) {
            let file = &self.graph[from].file;
            let language = crate::extract::language_for_extension(
                std::path::Path::new(file)
                    .extension()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or_default(),
            );
            for call in self.calls.get(&from).into_iter().flatten() {
                let count = self
                    .by_name
                    .get(target_name(call))
                    .into_iter()
                    .flatten()
                    .filter(|&&to| {
                        accepts(call, file, &self.graph[to].file, &self.graph[to].scope)
                            && crate::extract::language_for_extension(
                                std::path::Path::new(&self.graph[to].file)
                                    .extension()
                                    .and_then(std::ffi::OsStr::to_str)
                                    .unwrap_or_default(),
                            ) == language
                    })
                    .count();
                out.push(report(file, call, count));
            }
        }
        out.sort_by(|a, b| (&a.file, a.line, &a.callee).cmp(&(&b.file, b.line, &b.callee)));
        out
    }
}
