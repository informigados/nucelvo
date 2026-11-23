# Third-party notices

Nucelvo’s source code is covered by [Apache License 2.0](LICENSE). Third-party projects retain their own licenses and notices.

## Slint

Nucelvo uses Slint **1.16.1** under the **Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0**. The official Made with Slint badge appears in the public-facing [README](README.md). [Slint terms](https://slint.dev/terms-and-conditions#slint-royalty-free-license-2-0) · [Upstream release](https://github.com/slint-ui/slint/tree/v1.16.1)

## Dependency records

[Cargo.lock](Cargo.lock) records the versions of direct and transitive Rust dependencies. The package metadata for each locked dependency identifies its upstream license.

| Direct dependencies in the Windows or Linux build | Locked versions | License terms declared upstream |
| --- | --- | --- |
| arboard, chrono, directories | 3.7.0, 0.4.45, 6.0.0 | MIT or Apache 2.0 |
| futures, futures-timer, libc | 0.3.34, 3.0.4, 0.2.191 | MIT or Apache 2.0 |
| serde, serde_json, tempfile | 1.0.229, 1.0.154, 3.27.0 | MIT or Apache 2.0 |
| windows, windows-sys, wmi | 0.62.2, 0.61.2, 0.18.4 | MIT or Apache 2.0 |
| rfd, winreg | 0.18.0, 0.55.0 | MIT |
| csv | 1.4.0 | Unlicense or MIT |
| slint, slint-build | 1.16.1 | Slint Royalty-free License 2.0 for this project’s use |

The bundled [Made with Slint artwork](assets/brand/made-with-slint.png) is upstream Slint artwork used for attribution. [Official attribution artwork](https://github.com/slint-ui/slint/blob/v1.16.1/logo/MadeWithSlint-logo-whitebg.png)
