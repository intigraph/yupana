#!/usr/bin/env python3
"""Packaging/readback refusal controls; synthetic data is not native acceptance.

# arming: ci; release asset publication contract invokes this script.
"""

import importlib.util
import json
import os
import shutil
import subprocess
import sys
from unittest.mock import patch
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


builder = load("builder_test", ROOT / "build-repository-share.py")

publisher = load("publisher_test", ROOT / "publish_repository_share.py")
verifier = load("verifier_test", ROOT / "verify-repository-share.py")


class Publication(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.stage = self.root / "stage"
        self.stage.mkdir()
        recipe = {"model": "model.onnx", "model_sha256": "a" * 64, "tokenizer_sha256": "b" * 64, "dimension": 384}
        (self.stage / "manifest.json").write_text(json.dumps({"embedding": recipe}))
        for name in ("export.nt", "manifest.ttl", "shapes.ttl", "queries.ttl"):
            (self.stage / name).write_text("# synthetic fixture only\n")
        self.archive = self.root / "yupana-v1.2.3-repository.pendant.tar.gz"
        self.revision = "1" * 40

    def prepare(self):
        publisher.package(self.stage, self.archive)
        sha = publisher.digest(self.archive)
        self.archive.with_name(self.archive.name + ".sha256").write_text(sha + "  " + self.archive.name + "\n")
        self.proof = {
            "schema": "quipu.repository-share-producer/v1",
            "source_repository": "scbrown/yupana",
            "source_revision": self.revision,
            "release_tag": "v1.2.3",
            "archive_sha256": sha,
            "destination": "outward",
            "quipu_sha256": "2" * 64,
            "quipu_server_sha256": "5" * 64,
            "bobbin_sha256": "3" * 64,
            "engine_source_revision": "4" * 40,
        }
        self.proof_path = self.archive.with_name(self.archive.name + ".provenance.json")
        self.proof_path.write_text(json.dumps(self.proof))

    def test_deterministic_archive_and_exact_readback(self):
        self.prepare()
        other = self.root / "second.tar.gz"
        publisher.package(self.stage, other)
        self.assertEqual(self.archive.read_bytes(), other.read_bytes())
        self.assertEqual(verifier.verify(self.root, "v1.2.3", self.revision)["source_revision"], self.revision)

    def test_required_members_and_recipe(self):
        (self.stage / "queries.ttl").unlink()
        with self.assertRaisesRegex(ValueError, "required nonempty"):
            publisher.check_share(self.stage)
        (self.stage / "queries.ttl").write_text("# query")
        (self.stage / "manifest.json").write_text("{}")
        with self.assertRaisesRegex(ValueError, "provider-byte"):
            publisher.check_share(self.stage)

    def test_text_line_span_and_private_members_refuse(self):
        for predicate in ("content", "startLine", "endLine"):
            (self.stage / "export.nt").write_text(
                "<https://example.org/s> <http://aegis.gastown.local/ontology/" + predicate + '> "body" .\n'
            )
            with self.subTest(predicate=predicate), self.assertRaisesRegex(ValueError, "duplicates source"):
                publisher.check_share(self.stage)
        (self.stage / "export.nt").write_text("# empty synthetic")
        (self.stage / "policy-token").write_text("not a real token")
        with self.assertRaisesRegex(ValueError, "unexpected"):
            publisher.check_share(self.stage)

    def test_foreign_repository_and_wrong_source_refuse(self):
        self.prepare()
        for field, value in [
            ("source_repository", "scbrown/quipu"),
            ("source_revision", "f" * 40),
            ("release_tag", "v9.9.9"),
            ("archive_sha256", "f" * 64),
        ]:
            self.proof_path.write_text(json.dumps({**self.proof, field: value}))
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "provenance mismatch"):
                verifier.verify(self.root, "v1.2.3", self.revision)

    def test_checksum_refuses(self):
        self.prepare()
        self.archive.with_name(self.archive.name + ".sha256").write_text("wrong")
        with self.assertRaisesRegex(ValueError, "checksum"):
            verifier.verify(self.root, "v1.2.3", self.revision)

    def test_hold_absence_presence_symlink_and_unknown(self):
        path = self.root / "operator-hold"
        with patch.dict(os.environ, {"QUIPU_SHARE_HOLD_FILE": str(path)}):
            self.assertFalse(publisher.held())
            path.write_text("hold")
            self.assertTrue(publisher.held())
            path.unlink()
            path.symlink_to(self.root / "missing")
            self.assertTrue(publisher.held())
            with patch.object(Path, "lstat", side_effect=PermissionError("fixture")):
                with self.assertRaisesRegex(ValueError, "hold state unreadable"):
                    publisher.held()

    def test_wait_is_bounded(self):
        calls, pauses = [], []

        class Result:
            returncode = 1

        def run(args, **kwargs):
            calls.append(args)
            return Result()

        with self.assertRaisesRegex(ValueError, "bounded"):
            verifier.wait("v1.2.3", self.revision, attempts=2, pause=pauses.append, run=run, admitted=True)
        self.assertEqual(len(calls), 2)
        self.assertEqual(pauses, [30])

    def test_unadmitted_release_makes_no_read_or_wait(self):
        def forbidden(*args, **kwargs):
            self.fail("unadmitted release attempted a read or pause")

        result = verifier.wait("v1.2.3", self.revision, run=forbidden, pause=forbidden)
        self.assertEqual(result, {
            "status": "NOT_ADMITTED", "source_repository": "scbrown/yupana",
            "release_tag": "v1.2.3", "source_revision": self.revision,
        })
        with self.assertRaisesRegex(ValueError, "invalid release identity"):
            verifier.wait("bad-tag", self.revision, run=forbidden)
        with self.assertRaisesRegex(ValueError, "boolean"):
            verifier.wait("v1.2.3", self.revision, admitted="false", run=forbidden)

    def test_admitted_release_still_verifies_downloaded_bytes(self):
        self.prepare()
        calls = []

        def download(args, **kwargs):
            calls.append(args)
            folder = Path(args[args.index("--dir") + 1])
            for source in (self.archive, self.proof_path, Path(str(self.archive) + ".sha256")):
                shutil.copyfile(source, folder / source.name)
            return subprocess.CompletedProcess(args, 0)

        result = verifier.wait("v1.2.3", self.revision, attempts=1, run=download, admitted=True)
        self.assertEqual(result["status"], "VERIFIED")
        self.assertEqual(result["source_revision"], self.revision)
        self.assertEqual(len(calls), 1)
        self.archive.with_name(self.archive.name + ".sha256").write_text("wrong")
        with self.assertRaisesRegex(ValueError, "checksum"):
            verifier.wait("v1.2.3", self.revision, attempts=1, run=download, admitted=True)

    def test_admission_cli_reports_output_and_refuses_malformed_values(self):
        for value, status, output in [(None, "NOT_ADMITTED", "false"), ("", "NOT_ADMITTED", "false"),
                                      ("false", "NOT_ADMITTED", "false"), ("true", "ADMITTED", "true")]:
            with self.subTest(value=value):
                env = dict(os.environ)
                env.pop("TRUSTED_REPOSITORY_SHARE_ENABLED", None)
                if value is not None:
                    env["TRUSTED_REPOSITORY_SHARE_ENABLED"] = value
                target = self.root / "github-output"
                target.write_text("existing=retained\n")
                env["GITHUB_OUTPUT"] = str(target)
                result = subprocess.run([sys.executable, str(ROOT / "verify-repository-share.py"), "--admission"],
                                        env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)["status"], status)
                self.assertEqual(target.read_text(), "existing=retained\nadmitted=" + output + "\n")
        for value in ("TRUE", "1", " false "):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "admission"):
                verifier.publisher_admitted(value)

    def test_release_workflow_binds_admission_before_readback(self):
        workflow = (ROOT.parent / ".github/workflows/release.yml").read_text()
        job = workflow.split("  repository-pendant:\n", 1)[1]
        self.assertIn("TRUSTED_REPOSITORY_SHARE_ENABLED: ${{ vars.TRUSTED_REPOSITORY_SHARE_ENABLED }}", job)
        self.assertIn("id: publisher\n        run: python3 scripts/verify-repository-share.py --admission", job)
        self.assertIn("if: steps.publisher.outputs.admitted == 'true'", job)
        self.assertLess(job.index("--admission"), job.index("Verify the bounded public pointer corpus"))


class WorkerEnvironment(unittest.TestCase):
    def test_actual_command_and_mcp_child_do_not_inherit_host_configuration(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            host, private = root / "host", root / "private"
            host.mkdir()
            private.mkdir()
            (host / "host-marker").write_text("synthetic fixture")
            ambient = {
                "HOME": str(host), "XDG_CONFIG_HOME": str(host),
                "XDG_DATA_HOME": str(host), "XDG_CACHE_HOME": str(host),
                "XDG_STATE_HOME": str(host), "QUIPU_SERVER": "https://fixture.invalid",
                "QUIPU_AUTH_TOKEN": "synthetic", "BOBBIN_QUIPU_TOKEN": "synthetic",
                "BOBBIN_SERVER": "https://fixture.invalid", "BOBBIN_QUIPU_REMOTE": "fixture",
                "GH_TOKEN": "synthetic", "UNRECOGNIZED_SECRET": "synthetic",
                "HTTP_PROXY": "https://fixture.invalid", "PYTHONPATH": str(host),
            }
            probe = """import os,json,pathlib
keys = ["QUIPU_SERVER","QUIPU_AUTH_TOKEN","BOBBIN_QUIPU_TOKEN","BOBBIN_SERVER","BOBBIN_QUIPU_REMOTE","GH_TOKEN","UNRECOGNIZED_SECRET","HTTP_PROXY","PYTHONPATH"]
result = {"host_marker": (pathlib.Path.home()/"host-marker").exists(), "ambient_keys": [key for key in keys if key in os.environ], "home": str(pathlib.Path.home()), "xdg": {key: os.environ[key] for key in ("XDG_CONFIG_HOME","XDG_DATA_HOME","XDG_CACHE_HOME","XDG_STATE_HOME")}}
"""
            fake = private / "fake-mcp"
            fake.write_text("#!" + sys.executable + "\n" + probe + """
import sys
for line in sys.stdin:
    request=json.loads(line)
    if "id" not in request:
        continue
    response = {} if request["id"] == 0 else {"content":[{"type":"text","text":json.dumps(result)}]}
    print(json.dumps({"jsonrpc":"2.0","id":request["id"],"result":response}),flush=True)
""")
            fake.chmod(0o700)
            with patch.dict(os.environ, ambient):
                direct = json.loads(builder.command([sys.executable, "-c", probe + "print(json.dumps(result))"], cwd=private))
                (mcp,) = builder.local_mcp(fake, private / "test.db", [("fixture", {})])
                # Explicit caller environments remain available only when requested,
                # as required by the live-policy helper's reviewed exception.
                explicit = json.loads(builder.command([sys.executable, "-c", probe + "print(json.dumps(result))"], cwd=private, env=dict(os.environ)))
            self.assertTrue(explicit["host_marker"])
            self.assertIn("GH_TOKEN", explicit["ambient_keys"])
            for result in (direct, mcp):
                self.assertFalse(result["host_marker"])
                self.assertEqual(result["ambient_keys"], [])
                home = Path(result["home"])
                self.assertEqual(home.parent, private)
                self.assertEqual(home.stat().st_mode & 0o777, 0o700)
                for directory in result["xdg"].values():
                    self.assertEqual(Path(directory).parent, home)
                    self.assertEqual(Path(directory).stat().st_mode & 0o777, 0o700)


if __name__ == "__main__":
    unittest.main()
