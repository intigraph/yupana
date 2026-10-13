#!/usr/bin/env python3
"""Replay the same library inputs at two Git revisions in private offline fixtures."""
import argparse
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[3]


def replay(revision):
    source = subprocess.check_output(
        ["git", "show", f"{revision}:src/ci_shift.rs"], cwd=ROOT
    )
    exact = subprocess.check_output(
        ["git", "rev-parse", f"{revision}^{{commit}}"], cwd=ROOT, text=True
    ).strip()
    with tempfile.TemporaryDirectory(prefix="ci-map-demo-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "Cargo.toml").write_text(
            '[package]\nname="ci-map-demo"\nversion="0.0.0"\nedition="2021"\n'
            '[lib]\nname="yupana"\n[features]\nquipu=[]\n'
            '[dependencies]\nserde={version="1",features=["derive"]}\n'
            'serde_json="1"\nglob="0.3"\n'
        )
        (root / "src/ci_shift.rs").write_bytes(source)
        (root / "src/lib.rs").write_text(
            "pub mod errors {\n"
            " #[derive(Debug)] pub enum Error {Projection(String), Io(std::io::Error)}\n"
            " pub type Result<T> = std::result::Result<T,Error>;\n"
            " impl From<std::io::Error> for Error {\n"
            "  fn from(error:std::io::Error)->Self{Self::Io(error)}\n"
            " }\n}\npub mod ci_shift;\n"
        )
        (root / "src/main.rs").write_bytes(
            (ROOT / "examples/ci_map_validation_demo.rs").read_bytes()
        )
        manifest = str(root / "Cargo.toml")
        subprocess.run(
            ["cargo", "generate-lockfile", "--offline", "--manifest-path", manifest],
            check=True, capture_output=True, text=True,
        )
        result = subprocess.run(
            ["cargo", "run", "--quiet", "--offline", "--locked", "--manifest-path", manifest],
            check=True, capture_output=True, text=True,
        )
        print(f"revision {exact}", flush=True)
        print(result.stdout, end="", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base")
    parser.add_argument("head")
    args = parser.parse_args()
    replay(args.base)
    replay(args.head)
