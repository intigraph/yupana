//! Regression fixtures describe target identity and uncertainty, not implementation.
use super::*;

fn graph(files: &[(&str, &str)]) -> CodeGraph {
    CodeGraph::from_sources(files.iter().map(|(path, body)| {
        let language =
            crate::extract::language_for_extension(path.rsplit('.').next().unwrap()).unwrap();
        ((*path).to_owned(), (*body).to_owned(), language)
    }))
}
fn targets(graph: &CodeGraph, caller: &str) -> Vec<String> {
    let mut files: Vec<_> = graph
        .direct(caller, Dir::Callees)
        .into_iter()
        .map(|r| r.file)
        .collect();
    files.sort();
    files
}
const A: &str = "def leaf(): pass\n";
const B: &str = "def leaf(): pass\n";

#[test]
fn explicit_import_filters_target_and_preserves_positive_caller() {
    let g = graph(&[
        ("a.py", A),
        ("b.py", B),
        (
            "main.py",
            "from a import leaf\ndef invoke(): leaf()\ndef positive(): invoke()\n",
        ),
    ]);
    assert_eq!(targets(&g, "invoke"), ["a.py"]);
    assert_eq!(g.direct("invoke", Dir::Callers)[0].name, "positive");
    assert_eq!(g.call_resolution("invoke")[0].status, "import_candidate");
}

#[test]
fn aliases_module_receivers_and_relative_packages_keep_exact_target() {
    for source in [
        "from .a import leaf as chosen\ndef invoke(): chosen()\n",
        "import pkg.a as chosen\ndef invoke(): chosen.leaf()\n",
    ] {
        let g = graph(&[("pkg/a.py", A), ("pkg/b.py", B), ("pkg/main.py", source)]);
        assert_eq!(targets(&g, "invoke"), ["pkg/a.py"]);
        assert_eq!(g.call_resolution("invoke")[0].candidate_count, 1);
    }
}

#[test]
fn duplicate_caller_names_do_not_steal_calls_across_files_or_classes() {
    let g = graph(&[
        ("a.py", "def alpha(): pass\ndef same(): alpha()\n"),
        ("b.py", "def beta(): pass\ndef same(): beta()\n"),
    ]);
    let a = g
        .definitions("same")
        .into_iter()
        .find(|d| d.file == "a.py")
        .unwrap();
    let ix = g.by_name["same"]
        .iter()
        .copied()
        .find(|&ix| g.graph[ix].file == a.file)
        .unwrap();
    assert_eq!(
        g.graph
            .neighbors(ix)
            .map(|n| g.graph[n].name.as_str())
            .collect::<Vec<_>>(),
        ["alpha"]
    );
    let g=graph(&[("main.py","def alpha(): pass\ndef beta(): pass\nclass A:\n def same(self): alpha()\nclass B:\n def same(self): beta()\n")]);
    for &ix in &g.by_name["same"] {
        assert_eq!(g.graph.neighbors(ix).count(), 1);
    }
}

#[test]
fn ambiguous_imports_and_dynamic_bindings_are_explicit_conservative_candidates() {
    for source in [
        "from a import leaf\nfrom b import leaf\ndef invoke(): leaf()\n",
        "from a import leaf\ndef invoke(leaf): leaf()\n",
        "from a import leaf\ndef invoke():\n leaf = factory()\n leaf()\n",
        "from a import *\ndef invoke(): leaf()\n",
        "def invoke(receiver): receiver.leaf()\n",
    ] {
        let g = graph(&[("a.py", A), ("b.py", B), ("main.py", source)]);
        let rows = g.call_resolution("invoke");
        let row = rows.iter().find(|r| r.callee == "leaf").unwrap();
        assert_eq!(row.status, "unsupported", "{source}");
        assert_eq!(row.candidate_count, 2, "{source}");
    }
}

#[test]
fn unknown_import_is_unresolved_without_global_name_fallback() {
    let g = graph(&[
        ("b.py", B),
        ("main.py", "from absent import leaf\ndef invoke(): leaf()\n"),
    ]);
    assert!(targets(&g, "invoke").is_empty());
    assert_eq!(g.call_resolution("invoke")[0].status, "unresolved");
}

#[test]
fn unqualified_collision_is_ambiguous_and_never_cross_language() {
    let g = graph(&[
        ("a.py", A),
        ("b.py", B),
        ("other.rs", "fn leaf() {}"),
        ("main.py", "def invoke(): leaf()\n"),
    ]);
    assert_eq!(targets(&g, "invoke"), ["a.py", "b.py"]);
    assert_eq!(g.call_resolution("invoke")[0].status, "ambiguous");
}

#[test]
fn tenant_edits_use_same_import_filter_and_masked_definition_identity() {
    let dir = tempfile::tempdir().unwrap();
    for (path, body) in [
        ("a.py", A),
        ("b.py", B),
        ("main.py", "from a import leaf\ndef invoke(): leaf()\n"),
    ] {
        std::fs::write(dir.path().join(path), body).unwrap();
    }
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap()
            .status
            .success());
    }
    let base = Base::build_at(dir.path(), "HEAD").unwrap();
    let mut reg = TenantRegistry::new(base);
    reg.touch("one", "a.py", "def leaf():\n return 1\n");
    let files = |view: TenantView<'_>| {
        reachable_over(&view, "invoke", Dir::Callees, 1)
            .into_iter()
            .map(|r| r.file)
            .collect::<Vec<_>>()
    };
    assert_eq!(files(reg.view("one")), ["a.py"]);
    reg.touch(
        "one",
        "main.py",
        "from b import leaf as selected\ndef invoke(): selected()\n",
    );
    assert_eq!(files(reg.view("one")), ["b.py"]);
    assert_eq!(files(reg.view("two")), ["a.py"]);
    assert_eq!(
        reg.view("one").call_resolution("invoke")[0].status,
        "import_candidate"
    );
    let rows = reachable_over(&reg.view("one"), "leaf", Dir::Callers, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "invoke");
}

#[test]
fn module_import_does_not_match_a_class_method_with_the_same_name() {
    let g = graph(&[
        ("a.py", "class A:\n def leaf(self): pass\n"),
        ("main.py", "from a import leaf\ndef invoke(): leaf()\n"),
    ]);
    assert!(targets(&g, "invoke").is_empty());
    assert_eq!(g.call_resolution("invoke")[0].status, "unresolved");
}

#[test]
fn self_recursion_has_a_candidate_even_though_walks_exclude_the_seed() {
    let g = graph(&[("main.py", "def invoke(): invoke()\n")]);
    assert!(targets(&g, "invoke").is_empty());
    assert_eq!(g.call_resolution("invoke")[0].candidate_count, 1);
}

#[test]
fn decorated_methods_skip_class_imports_but_keep_enclosing_function_closures() {
    for source in [
        "from b import leaf\nclass A:\n from a import leaf\n @decorate\n def invoke(self): leaf()\n",
        "def outer():\n from b import leaf\n class A:\n  from a import leaf\n  @decorate\n  def invoke(self): leaf()\n",
    ] {
        let g=graph(&[("a.py",A),("b.py",B),("main.py",source)]);
        assert_eq!(targets(&g,"invoke"),["b.py"]);
    }
}
