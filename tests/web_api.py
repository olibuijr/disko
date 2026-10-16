#!/usr/bin/env python3
import os
from pathlib import Path
import time
import unittest
from http_support import Server

class DashboardTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = Server()
        cls.server.login()

    @classmethod
    def tearDownClass(cls):
        cls.server.close()

    def test_authentication_and_csrf(self):
        status, _, _ = self.server.request('GET', '/api/status', authenticated=False)
        self.assertEqual(status, 401)
        status, _, _ = self.server.request('POST', '/api/login', {'password': 'wrong'}, authenticated=False)
        self.assertEqual(status, 401)
        status, _, _ = self.server.request('POST', '/api/rescan', headers={'X-Disko-Token': 'wrong'})
        self.assertEqual(status, 403)
        status, _, _ = self.server.request('GET', '/api/status', headers={'Origin': 'http://attacker.invalid'})
        self.assertEqual(status, 403)
        status, _, _ = self.server.request('GET', '/api/status', headers={'Host': f'attacker.invalid:{self.server.port}'})
        self.assertEqual(status, 403)

    def test_pages_and_api(self):
        for path in ['/', '/js/app.js', '/js/tree.js', '/styles/base.css', '/styles/auth.css']:
            status, _, headers = self.server.request('GET', path)
            self.assertEqual(status, 200, path)
            self.assertIn('Content-Security-Policy', headers)
        for path in ['/api/status', '/api/roots', '/api/history', '/api/activity', '/api/tree', '/api/largest']:
            self.assertEqual(self.server.request('GET', path)[0], 200, path)

    def preview(self, path):
        entry = self.server.entry(path)
        status, preview, _ = self.server.request('POST', '/api/delete/preview', {'id': entry['id']})
        self.assertEqual(status, 200, preview)
        return preview

    def test_delete_tmp_file(self):
        path = self.server.root / 'delete-file'
        path.write_bytes(b'temporary')
        preview = self.preview(path)
        status, _, _ = self.server.request('POST', '/api/delete', {'token': preview['token']})
        self.assertEqual(status, 200)
        self.assertFalse(path.exists())
        self.assertEqual(self.server.request('POST', '/api/delete', {'token': preview['token']})[0], 400)

    def test_recursive_delete_never_follows_symlinks(self):
        outside = self.server.base / 'keep'
        outside.mkdir()
        sentinel = outside / 'sentinel'
        sentinel.write_bytes(b'keep')
        path = self.server.root / 'delete-folder'
        (path / 'nested').mkdir(parents=True)
        (path / 'nested/file').write_bytes(b'temporary')
        os.symlink(outside, path / 'link')
        preview = self.preview(path)
        self.assertEqual(self.server.request('POST', '/api/delete', {'token': preview['token']})[0], 200)
        self.assertFalse(path.exists())
        self.assertEqual(sentinel.read_bytes(), b'keep')

    def test_preview_inode_replacement_is_rejected(self):
        path = self.server.root / 'replace-me'
        path.write_bytes(b'original')
        preview = self.preview(path)
        replacement = self.server.root / 'replacement'
        replacement.write_bytes(b'new')
        os.replace(replacement, path)
        self.assertEqual(self.server.request('POST', '/api/delete', {'token': preview['token']})[0], 400)
        self.assertEqual(path.read_bytes(), b'new')

    def test_index_root_cannot_be_deleted(self):
        entry = self.server.entry(self.server.root)
        status, _, _ = self.server.request('POST', '/api/delete/preview', {'id': entry['id']})
        self.assertEqual(status, 400)
        self.assertTrue(self.server.root.is_dir())

    def test_prefix_search_and_history(self):
        status, rows, _ = self.server.request('GET', '/api/find?q=sample')
        self.assertEqual(status, 200)
        self.assertTrue(any(row['path'].endswith('sample.bin') for row in rows))
        _, history, _ = self.server.request('GET', '/api/history')
        self.assertTrue(history['scans'])
        self.assertTrue(history['samples'])

if __name__ == '__main__':
    unittest.main(verbosity=2)
