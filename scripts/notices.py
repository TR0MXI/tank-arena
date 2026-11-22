#!/usr/bin/env python3
"""按本地 Cargo 依赖图收集发行包声明，包含内嵌 C/C++ 组件的许可证。"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]

PREFIXES = ('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'COPYRIGHT')
EXCLUDED_EXTENSIONS = {'.rs', '.c', '.h', '.py', '.png'}


def build_dependencies(resolve):
    nodes = {node['id']: node for node in resolve['nodes']}
    visited, pending = set(), [resolve['root']]
    while pending:
        key = pending.pop()
        if key in visited:
            continue
        visited.add(key)
        for dependency in nodes[key]['deps']:
            if any(kind['kind'] != 'dev' for kind in dependency['dep_kinds']):
                pending.append(dependency['pkg'])
    return visited


def copy_licenses(package, destination):
    base = Path(package['manifest_path']).parent
    paths = {path for path in base.rglob('*')
             if path.name.upper().startswith(PREFIXES) and path.suffix.lower() not in EXCLUDED_EXTENSIONS
             and path.is_file() and not path.is_symlink()}
    if package.get('license_file'):
        paths.add(base / package['license_file'])

    for source in sorted(paths):
        target = destination / source.relative_to(base)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)


def collect(out):
    command = ['cargo', 'metadata', '--locked', '--offline', '--format-version=1',
               '--filter-platform=aarch64-apple-darwin']
    metadata = json.loads(subprocess.check_output(command, cwd=ROOT))
    needed = build_dependencies(metadata['resolve'])
    out.mkdir(parents=True, exist_ok=True)
    lines = ['# Tank Arena · Third-party notices', '',
             'Native macOS build and build-time dependencies. See files by component for upstream terms.', '']

    for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
        if package['id'] not in needed or package['source'] is None:
            continue
        name = f"{package['name']}-{package['version']}"
        copy_licenses(package, out / name)
        license_name = package.get('license') or 'see upstream license'
        url = package.get('repository') or package.get('homepage') or ''
        lines.append(f'- {name}: {license_name} — {url}')
    (out / 'INDEX.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='第三方许可证输出目录')
    collect(parser.parse_args().output)
