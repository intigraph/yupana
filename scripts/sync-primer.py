#!/usr/bin/env python3
"""Generate the skill and mdBook primer from the embedded Rust source."""
from pathlib import Path
root = Path(__file__).resolve().parents[1]
primer = (root / "src/primer.md").read_text()
skill = root / "skills/yupana/SKILL.md"
skill.parent.mkdir(parents=True, exist_ok=True)
skill.write_text("---\nname: yupana\ndescription: Orient agents and inspect code structure with Yupana.\n---\n\n<!-- BEGIN yupana prime -->\n" + primer + "<!-- END yupana prime -->\n")
(root / "docs/book/src/getting-started/agent-primer.md").write_text(primer)
