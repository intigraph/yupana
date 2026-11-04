#!/usr/bin/env python3
"""Inspect actual YAML and a prefix-selection fixture; no Actions API calls."""
from copy import deepcopy
from pathlib import Path
import subprocess
import yaml
ROOT = Path(__file__).resolve().parents[3]
BASE = 'ba1811278903e7f3c5d3f23c993027588deb2a22'
base = yaml.safe_load(subprocess.check_output(['git','show',BASE+':.github/workflows/ci.yml'], cwd=ROOT))
head = yaml.safe_load((ROOT/'.github/workflows/ci.yml').read_text())

def cache(config, job):
    return next(s['with'] for s in config['jobs'][job]['steps'] if s.get('uses') == 'actions/cache@v4')

def without_caches(config):
    value = deepcopy(config)
    for job in value['jobs'].values():
        job['steps'] = [s for s in job.get('steps',[]) if s.get('uses') != 'actions/cache@v4']
    return value

assert without_caches(base) == without_caches(head), 'non-cache job behavior changed'
print('BASE '+BASE+' -> CANDIDATE source-cache configuration')
for name in ('clippy','test','build'):
    before, after = cache(base,name), cache(head,name)
    print(name+': '+repr(before['path'].splitlines())+' -> '+repr(after['path'].splitlines()))
    assert before['path'].splitlines()[-1] == 'target'
    assert after['path'].splitlines() == ['~/.cargo/registry','~/.cargo/git']
    assert '-cargo-sources-v1-' in after['key']
    assert all('-cargo-sources-v1-' in k for k in after['restore-keys'].splitlines())

# This illustrates the documented prefix rule; it is not a hosted cache restore.
archive = 'Linux-cargo-clippy-quipu-example-lock'
def selected(config):
    prefixes = cache(config,'test')['restore-keys'].replace('${{ runner.os }}','Linux').replace('${{ matrix.name }}','mcp+golden-path').splitlines()
    return next((archive for prefix in prefixes if archive.startswith(prefix)), None)
print('Cross-feature archive fixture:', archive)
print('Base fallback:', selected(base))
print('Candidate fallback:', selected(head))
assert selected(base) == archive and selected(head) is None
print('PASS: all non-cache job configuration and feature matrices preserved')
print('LIMIT: cold hosted restore and full matrix execution remain required')
