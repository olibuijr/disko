#!/usr/bin/env python3
"""Root-only mount tests on disposable loop images, entirely under /tmp.

Covers: filesystems mounted and unmounted while the daemon runs (and that it
never keeps a mount busy), Btrfs subvolumes watched through the detached
top-level mark, relocated bind mounts not being counted twice, and deletion
refusing to remove a nested subvolume.
"""
import os
from pathlib import Path
import subprocess
import tempfile
import time
from http_support import Server

if os.geteuid() != 0:
    raise SystemExit('Run with sudo; this test mounts ext4 and Btrfs fixtures under /tmp.')


def run(*args):
    subprocess.run(args, check=True, capture_output=True)


def image(path, mkfs):
    with path.open('wb') as file:
        file.truncate(256 * 1024 * 1024)
    run(*mkfs, str(path))


def until(predicate, what, timeout=15):
    deadline = time.monotonic() + timeout
    while not predicate():
        if time.monotonic() > deadline:
            raise AssertionError(f'timed out waiting for {what}')
        time.sleep(.1)


def indexed(server, path):
    from urllib.parse import urlencode
    status, data, _ = server.request('GET', '/api/info?' + urlencode({'path': str(path)}))
    return data if status == 200 else None


with tempfile.TemporaryDirectory(prefix='disko-mounts-', dir='/tmp') as temp:
    base = Path(temp)
    ext4, btrfs = base / 'ext4.img', base / 'btrfs.img'
    image(ext4, ['mkfs.ext4', '-q', '-F'])
    image(btrfs, ['mkfs.btrfs', '-q', '-f'])
    loop = subprocess.run(['losetup', '--find', '--show', str(btrfs)], check=True, capture_output=True, text=True).stdout.strip()
    top = base / 'top'
    top.mkdir()
    run('mount', loop, str(top))
    run('btrfs', 'subvolume', 'create', str(top / '@data'))
    run('btrfs', 'subvolume', 'create', str(top / '@data' / 'nested'))
    (top / '@data' / 'nested' / 'inner').write_bytes(b'n' * 300)
    run('umount', str(top))
    mounted = []
    server = None
    try:
        server = Server(base_dir=str(base))
        server.login()
        tree = server.root

        # A filesystem mounted inside the tree while the daemon runs.
        disk = tree / 'disk'
        disk.mkdir()
        run('mount', '-o', 'loop,nosuid,nodev,noexec', str(ext4), str(disk))
        mounted.append(disk)
        (disk / 'payload').write_bytes(b'p' * 4096)
        until(lambda: indexed(server, disk / 'payload'), 'file on newly mounted ext4')

        # A bind mount showing that disk's folder at another path is not counted twice.
        (disk / 'photos').mkdir()
        (disk / 'photos' / 'one').write_bytes(b'o' * 5000)
        alias = tree / 'alias'
        alias.mkdir()
        run('mount', '--bind', str(disk / 'photos'), str(alias))
        mounted.append(alias)
        until(lambda: indexed(server, disk / 'photos' / 'one'), 'file in bound folder')
        time.sleep(3)
        assert indexed(server, alias / 'one') is None, 'relocated bind mount was descended'

        # Unmounting must succeed while Disko runs, and the contents disappear.
        run('umount', str(alias))
        mounted.remove(alias)
        run('umount', str(disk))
        mounted.remove(disk)
        until(lambda: indexed(server, disk / 'payload') is None, 'unmounted contents to leave the index')

        # A Btrfs subvolume (not the top level) mounted inside the tree.
        sub = tree / 'sub'
        sub.mkdir()
        run('mount', '-o', 'subvol=@data', loop, str(sub))
        mounted.append(sub)
        until(lambda: indexed(server, sub / 'nested' / 'inner'), 'Btrfs subvolume contents')
        status = server.request('GET', '/api/status')[1]
        assert 'fanotify' in status['watch_backend'], status
        (sub / 'live').write_bytes(b'l' * 1234)
        until(lambda: (indexed(server, sub / 'live') or {}).get('size') == 1234, 'Btrfs event')
        (sub / 'nested' / 'deeper').write_bytes(b'd' * 77)
        until(lambda: indexed(server, sub / 'nested' / 'deeper'), 'event in nested subvolume')

        # Deleting a folder that contains a nested subvolume is refused untouched.
        folder = indexed(server, sub)
        assert folder is not None
        (sub / 'plain').mkdir()
        (sub / 'plain' / 'keep').write_bytes(b'k')
        until(lambda: indexed(server, sub / 'plain' / 'keep'), 'plain folder')
        nested = indexed(server, sub / 'nested')
        preview = server.request('POST', '/api/delete/preview', {'id': nested['id']})
        if preview[0] == 200:
            code, result, _ = server.request('POST', '/api/delete', {'token': preview[1]['token']})
            assert code != 200, result
        assert (sub / 'nested' / 'inner').exists(), 'nested subvolume contents were deleted'
        print('mount, unmount, bind, Btrfs subvolume and deletion boundary checks passed')
    finally:
        if server is not None:
            server.process.terminate()
            server.process.wait(timeout=15)
        for point in reversed(mounted):
            subprocess.run(['umount', '-l', str(point)], check=False)
        subprocess.run(['losetup', '-d', loop], check=False)
        if server is not None:
            server.close()
