# Contributing

Disko is free and open source under the [MIT licence](LICENSE). Use it, fork it,
change it and ship it. Issues and pull requests are welcome.

- **Bugs and ideas:** open an issue with what you ran, what you expected and what
  happened. For the daemon, `journalctl -u disko` and `disko status` help.
- **Pull requests:** keep each one to a single change, add a test when behaviour
  changes, and run the checks in [docs/development.md](docs/development.md)
  before you push (`cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`,
  `python3 tools/check_structure.py`). Application source files stay under 200 lines.
- **Security issues:** please report them privately through GitHub's security
  advisories rather than a public issue. See [docs/security.md](docs/security.md).
- **The website** is the static page in [site/](site/), published to GitHub Pages
  on every push to `main`.
