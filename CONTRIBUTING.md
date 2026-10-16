# Contributing

Issues and pull requests are welcome — bug reports, game descriptors,
integration questions, documentation.

* **Add a game**: a descriptor in [`games/`](games), validated with
  `cargo run -p warpzone-server -- check-games games`. See
  [docs/descriptors.md](docs/descriptors.md).
* **Code**: read [docs/development.md](docs/development.md). Before a pull
  request: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`.
* **API changes** start in `crates/protocol`, then the server, then
  `docs/api.md`, in the same pull request.
* **Security issues**: please use a private
  [security advisory](https://github.com/jarvis-jhk/warpzone/security/advisories/new),
  not a public issue.

By contributing you agree that your contribution is licensed under MIT OR
Apache-2.0, like the rest of the project.
