//! Paired regressions for the independent binding review.
#![cfg(feature = "langs-extra")]
use yupana::http_candidates::extract;

fn resolution(source: &str) -> (String, Vec<String>) {
    let file = extract(source, "a.py").unwrap();
    assert!(file.diagnostics.is_empty(), "{:?}", file.diagnostics);
    assert_eq!(file.candidates.len(), 1, "{source}");
    let c = &file.candidates[0];
    (c.resolution.clone(), c.unresolved_reasons.clone())
}
#[test]
fn every_parameter_form_shadows_the_imported_http_module() {
    for param in [
        "requests",
        "*requests",
        "**requests",
        "requests: object",
        "*requests: tuple",
        "**requests: dict",
        "requests=None",
        "requests: object=None",
    ] {
        let src = format!(
            "import requests\ndef f({param}): return requests.get('https://example.com/a')\n"
        );
        let (r, reasons) = resolution(&src);
        assert_eq!(r, "unresolved", "{param}");
        assert!(reasons.iter().any(|r| r == "ambiguous_binding"), "{param}");
    }
    for params in ["", "other", "*other", "**other"] {
        let src = format!(
            "import requests\ndef f({params}): return requests.get('https://example.com/a')\n"
        );
        assert_eq!(resolution(&src).0, "literal", "{params}");
    }
    for param in ["requests", "*requests", "**requests"] {
        let src =
            format!("import requests\nf=lambda {param}: requests.get('https://example.com/a')\n");
        assert_eq!(resolution(&src).0, "unresolved", "lambda {param}");
    }
}
#[test]
fn registration_requires_a_stable_module_handler_binding() {
    let prefix = "from fastapi import FastAPI\napp=FastAPI()\ndef handler(): pass\n";
    let call = "app.add_api_route('/a', handler, methods=['GET'])\n";
    assert_eq!(resolution(&(prefix.to_owned() + call)).0, "literal");
    for rebind in [
        "handler=other\n",
        "handler,other=pair\n",
        "*handler,other=pair\n",
        "handler += other\n",
        "import handler\n",
        "import other as handler\n",
        "from other import handler\n",
        "del handler\n",
        "for handler in handlers: pass\n",
        "with acquire() as handler: pass\n",
        "try: pass\nexcept Exception as handler: pass\n",
        "match value:\n case handler: pass\n",
        "def f(handler): pass\n",
        "def f(*handler): pass\n",
        "def f(**handler): pass\n",
        "from arbitrary import *\n",
    ] {
        let (r, reasons) = resolution(&(prefix.to_owned() + rebind + call));
        assert_eq!(r, "unresolved", "{rebind}");
        assert!(reasons.iter().any(|r| r == "unknown_handler"), "{rebind}");
    }
    let nested="from fastapi import FastAPI\napp=FastAPI()\ndef outer():\n def handler(): pass\n app.add_api_route('/a', handler, methods=['GET'])\n";
    assert_eq!(resolution(nested).0, "unresolved");
}
#[test]
fn decorator_keeps_its_direct_handler_and_wildcards_invalidate_module_imports() {
    let src="from fastapi import FastAPI\napp=FastAPI()\n@app.get('/a')\ndef handler(): pass\nhandler=other\n";
    assert_eq!(resolution(src).0, "literal");
    let src = "import requests\nfrom arbitrary import *\nrequests.get('https://example.com/a')\n";
    assert_eq!(resolution(src).0, "unresolved");
}
