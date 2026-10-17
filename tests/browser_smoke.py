#!/usr/bin/env python3
"""Browser checks. All create/delete operations are confined to /tmp fixtures."""
from pathlib import Path
import re
from http_support import Server
from playwright.sync_api import sync_playwright

server = Server()
assert str(server.base).startswith('/tmp/')
for folder, size in [('Projects', 192_000), ('Downloads', 128_000), ('Cache', 64_000), ('Documents', 32_000)]:
    path = server.root / folder
    path.mkdir()
    (path / 'example.bin').write_bytes(b'x' * size)
try:
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path='/usr/bin/chromium', headless=True, args=['--no-sandbox'])
        page = browser.new_page(viewport={'width': 1440, 'height': 1050}, device_scale_factor=1)
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        page.on('console', lambda message: errors.append(message.text) if message.type == 'error' else None)
        page.goto(f'http://127.0.0.1:{server.port}')
        page.get_by_label('Password', exact=True).fill('disko')
        page.get_by_role('button', name='Sign in').click()
        page.wait_for_selector('body.authenticated')
        page.wait_for_selector('.ledger')
        page.locator('[data-page="tree"]').click()
        page.wait_for_selector('.tree-tile')
        page.get_by_role('button', name='Select disk or index').click()
        page.get_by_role('option').first.click()
        page.wait_for_selector('.tree-tile')
        assert page.locator('.tree-tile').count() >= 4
        image = Path('/tmp/disko-disk-tree.png')
        page.screenshot(path=str(image), full_page=True)
        page.locator('[data-page="explorer"]').click()
        page.locator('tbody tr').filter(has_text='sample.bin').get_by_role('button').click()
        page.get_by_role('button', name='Delete', exact=True).click()
        page.get_by_role('dialog').wait_for()
        page.get_by_role('button', name='Cancel', exact=True).click()
        assert (server.root / 'sample.bin').exists()
        page.get_by_role('button', name='Delete', exact=True).click()
        page.get_by_role('button', name='Delete file', exact=True).click()
        page.get_by_role('dialog').wait_for(state='detached')
        assert not (server.root / 'sample.bin').exists()
        page.locator('[data-page="history"]').click()
        page.get_by_role('heading', name='Scans', exact=True).wait_for()
        page.set_viewport_size({'width': 390, 'height': 844})
        page.locator('[data-page="tree"]').click()
        page.wait_for_selector('.tree-tile')
        assert page.locator('html').evaluate('(node) => node.scrollWidth <= innerWidth + 1')
        page.screenshot(path='/tmp/disko-mobile.png', full_page=True)
        page.get_by_role('button', name='Sign out', exact=True).click()
        page.wait_for_selector('body:not(.authenticated)')
        assert not errors, errors
        browser.close()
        print(f'Browser checks passed. Screenshot: {image}')
finally:
    server.close()
