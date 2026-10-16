#!/usr/bin/env python3
"""Root-only fanotify test on a disposable ext4 image, entirely under /tmp."""
import os
from pathlib import Path
import subprocess
import tempfile
import time
from http_support import Server

if os.geteuid() != 0:
    raise SystemExit('Run with sudo; this test mounts an ext4 fixture under /tmp.')
with tempfile.TemporaryDirectory(prefix='disko-fanotify-', dir='/tmp') as temp:
    base = Path(temp)
    image = base / 'fixture.img'
    with image.open('wb') as file:
        file.truncate(64 * 1024 * 1024)
    subprocess.run(['mkfs.ext4', '-q', '-F', str(image)], check=True)
    mount = base / 'mount'
    mount.mkdir()
    subprocess.run(['mount', '-o', 'loop,nosuid,nodev,noexec', str(image), str(mount)], check=True)
    server = None
    try:
        server = Server(base_dir=str(mount))
        server.login()
        status = server.request('GET', '/api/status')[1]
        assert 'fanotify' in status['watch_backend'], status
        path = server.root / 'new-file'
        path.write_bytes(b'fanotify')
        assert server.entry(path)['size'] == 8
        folder = server.root / 'new-folder'
        folder.mkdir()
        (folder / 'nested').write_bytes(b'nested')
        assert server.entry(folder / 'nested')['size'] == 6
        moved = server.root / 'moved'
        folder.rename(moved)
        assert server.entry(moved / 'nested')['size'] == 6
        preview = server.request('POST', '/api/delete/preview', {'id': server.entry(moved)['id']})[1]
        status, result, _ = server.request('POST', '/api/delete', {'token': preview['token']})
        assert status == 200, result
        assert not moved.exists()
        print('fanotify create, nested create, rename and deletion passed on /tmp ext4 fixture')
    finally:
        if server is not None:
            server.close()
        subprocess.run(['umount', str(mount)], check=True)
