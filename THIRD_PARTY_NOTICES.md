# Third-party notices

The Muse composer, message-bubble and attachment selectors used by
`assets/muse-studio.js` were informed by MUSE2API:
https://github.com/czg86389-hub/muse2api

MIT License

Copyright (c) 2026 MUSE2API Authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Embedded proxy components

The SOCKS5 client uses fast-socks5 1.0.0 (MIT), pinned to the published crate
with source revision bae2fc05f993d3a247b5435ba7da221f3fd80a3c.
Cancellation and owned connection-task shutdown use tokio-util 0.7.20 (MIT)
on the existing Tokio runtime. New transitive components are tokio-stream
0.1.19 (MIT) and socket2 0.5.10 (MIT OR Apache-2.0).
Their original license notices are included under LICENSES/.
No independent proxy engine executable or Go runtime is distributed.

Fixed subscription nodes use the embedded shadowsocks 1.23.2 TCP client
(MIT, https://github.com/shadowsocks/shadowsocks-rust), shadowsocks-crypto
and its AEAD dependencies. Clash YAML uses serde_yaml_ng 0.10.0
(MIT, https://github.com/acatton/serde-yaml-ng) and unsafe-libyaml.
License texts for the additional Windows dependencies are under LICENSES/.
No protocol executable, plugin, external provider or script is downloaded.

Official sources:
https://github.com/dizda/fast-socks5
https://github.com/tokio-rs/tokio
https://github.com/rust-lang/socket2

## Application updater

The application updater uses ureq 3.5.2 (MIT OR Apache-2.0),
ureq-proto 0.7.1 (MIT OR Apache-2.0), zip 6.0.0 (MIT),
utf8-zero 0.8.1, webpki-roots 1.0.12, and zopfli 0.8.4.
Their bundled license texts are under LICENSES/. semver uses the existing
MIT OR Apache-2.0 dependency; SHA256 uses the already bundled ring library.
Official source/API references:
https://github.com/algesten/ureq
https://github.com/zip-rs/zip2
https://docs.github.com/en/rest/releases/releases#get-the-latest-release

The updater's HTTPS source is the public Kongbai333/muse-desktop repository.
A matching SHA256 protects download integrity; it is not an independent
publisher signature.
