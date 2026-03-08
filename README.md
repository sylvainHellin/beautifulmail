# beautifulmail (Archived)

> **This project has been merged into [`email`](https://github.com/sylvainHellin/email) as of v0.5.0.**
> The TUI is now built into the email CLI binary -- run `email` with no arguments to launch it.
> This repository is archived and no longer maintained.

---

A polished terminal user interface for managing email, built with Rust.

beautifulmail was a standalone TUI frontend for the [`email`](https://github.com/sylvainHellin/email) CLI. It provided a three-panel email client experience directly in the terminal -- sidebar, email list, and preview -- while delegating all backend operations (fetch, send, reply, archive) to the `email` binary.

As of v0.5.0, the TUI has been integrated directly into the email CLI as a library, eliminating subprocess calls and enabling direct function invocations for all operations.

## Migration

Install the latest email CLI:

```bash
cd /path/to/email
cargo install --path .
```

Then simply run `email` with no arguments to launch the TUI.

## License

MIT -- see [LICENSE](./LICENSE).
