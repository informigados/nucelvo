# Contributing to Nucelvo

## 🚀 Getting started

Follow the [build instructions](README.md#-run-from-source) and review the [compatibility notes](docs/compatibility.md) before working on hardware support. Please use English in issues, pull requests, and code comments. For language changes, see the [translation guide](translations/README.md).

## 🐛 Reporting a problem

Include the operating system and version, hardware model if relevant, steps to reproduce the issue, and what you expected to see. If you attach a report, use **Share-safe** and review its contents before posting.

For suspected vulnerabilities, follow the [security policy](SECURITY.md).

## 🧪 Checking a change

Run these commands from the project root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run --locked -- --smoke-test
```

For collector changes, run the smoke test with `--require-logical-cpu` on a machine that exposes per-processor counters. Open the affected screen in English and Brazilian Portuguese after changing visible text.

For hardware changes, include the source, device and OS used for validation. Document conditions such as unavailable data, permission limits, and driver requirements. Keep JSON and CSV field names, units, and status codes stable when changing their presentation.

## 🔀 Opening a pull request

Describe the behavior change, the platforms you checked, and any validation that remains pending. Add a screenshot for visual changes when it helps review. Remove identifying information from screenshots and fixtures.
