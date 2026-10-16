"""Temporary test server and HTTP client. Every writable fixture lives in /tmp."""
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time

BINARY = Path(os.environ.get('DISKO_BIN', Path(__file__).parents[1] / 'target/debug/disko'))

class Server:
    def __init__(self, base_dir="/tmp"):
        self.temp = tempfile.TemporaryDirectory(prefix='disko-http-', dir=base_dir)
        self.base = Path(self.temp.name)
        self.root = self.base / 'tree'
        self.root.mkdir()
        self.state = self.base / 'state'
        self.state.mkdir()
        self.db = self.state / 'index.db'
        (self.root / 'sample.bin').write_bytes(b'x' * 8192)
        (self.root / 'folder').mkdir()
        (self.root / 'folder/child').write_bytes(b'hello')
        subprocess.run([str(BINARY), '--db', str(self.db), 'password'], input=b'disko\n', check=True, capture_output=True)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            self.port = sock.getsockname()[1]
        self.log = (self.base / 'daemon.log').open('w+')
        self.process = subprocess.Popen([str(BINARY), '--db', str(self.db), 'watch', str(self.root), '--listen', f'127.0.0.1:{self.port}', '--reconcile-secs', '0'], stdout=self.log, stderr=self.log)
        self.cookie = ''
        self.csrf = ''
        deadline = time.monotonic() + 15
        while True:
            try:
                status, _, _ = self.request('GET', '/api/session', authenticated=False)
                if status == 200:
                    break
            except OSError:
                pass
            if time.monotonic() > deadline:
                self.log.seek(0)
                raise RuntimeError(self.log.read())
            time.sleep(.05)

    def request(self, method, path, body=None, authenticated=True, headers=None):
        connection = http.client.HTTPConnection('127.0.0.1', self.port, timeout=15)
        values = {'Content-Type': 'application/json'}
        if authenticated:
            values['Cookie'] = self.cookie
            values['X-Disko-Token'] = self.csrf
        values.update(headers or {})
        connection.request(method, path, json.dumps(body) if body is not None else None, values)
        response = connection.getresponse()
        raw = response.read()
        result_headers = dict(response.getheaders())
        content_type = response.getheader('Content-Type', '')
        data = json.loads(raw) if 'application/json' in content_type else raw.decode()
        status = response.status
        connection.close()
        return status, data, result_headers

    def login(self):
        status, data, headers = self.request('POST', '/api/login', {'password': 'disko'}, authenticated=False)
        assert status == 200, data
        self.cookie = headers['Set-Cookie'].split(';')[0]
        self.csrf = data['token']

    def entry(self, path):
        from urllib.parse import urlencode
        deadline = time.monotonic() + 10
        while True:
            status, data, _ = self.request('GET', '/api/info?' + urlencode({'path': str(path)}))
            if status == 200:
                return data
            if time.monotonic() > deadline:
                raise AssertionError(data)
            time.sleep(.05)

    def close(self):
        self.process.terminate()
        self.process.wait(timeout=15)
        self.log.close()
        self.temp.cleanup()
