# Public synthetic interoperability fixtures

Everything here is test-only, including the passphrases, keys, Cookie values and `.invalid` proxy.
No user session is permitted in this directory.

`generate-reference.py` uses Python hashlib.pbkdf2_hmac and cryptography AESGCM independently
of the Java implementation. `payload.json` is exact UTF-8 plaintext; ASCII/Unicode `.properties`
files give UTF-8 passphrase Base64, salt/nonce, derived key and SHA-256 hashes.
`.muse-account` files use the canonical envelope in `docs/protocols/account-transfer-v1.md`.
Do not regenerate reference fixtures merely to make a failing implementation pass.

Java checks separately create a fresh OS-random synthetic envelope in the ignored check output.
Java- and Rust-generated exchange fixtures must record the actual producer/consumer and SHA-256.
