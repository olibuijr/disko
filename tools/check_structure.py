#!/usr/bin/env python3
"""Keep application source in focused files; check version consistency."""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
LIMIT = 200
errors = []
for directory in ['src', 'web']:
    for path in sorted((ROOT / directory).rglob('*')):
        if path.suffix not in {'.rs', '.js', '.css', '.html'}:
            continue
        lines = len(path.read_text().splitlines())
        if lines > LIMIT:
            errors.append(f'{path.relative_to(ROOT)}: {lines} lines (limit {LIMIT})')
version = (ROOT / 'VERSION').read_text().strip()
manifest = re.search(r'^version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text(), re.M).group(1)
if version != manifest:
    errors.append('VERSION and Cargo.toml disagree')
if f'## [{version}]' not in (ROOT / 'CHANGELOG.md').read_text():
    errors.append('Current version is missing from CHANGELOG.md')
if f'version-{version}-' not in (ROOT / 'README.md').read_text():
    errors.append('README version badge is stale')
if errors:
    print('\n'.join(errors), file=sys.stderr)
    sys.exit(1)
print(f'Structure and version checks passed (application-source limit: {LIMIT} lines).')
