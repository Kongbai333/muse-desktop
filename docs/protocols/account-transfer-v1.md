# Muse Account Transfer v1 — frozen 2026-11-22

This document is the sole frozen v1 wire authority for Android and PC. Java JCA and Rust ring reproduced the independent fixtures and consumed each other's newly generated Unicode packages on 2026-11-22; evidence is in the fixture interoperability record. No client may publish a divergent v1. All fixture credentials are synthetic. This freeze does not claim Android-device or real-account validation.

## 1. Encrypted envelope

File extension `.muse-account`; UTF-8 ASCII JSON, no BOM/newline/whitespace. **Exact field order and spelling**:

```text
{"format":"muse-account-transfer","version":1,"kdf":"PBKDF2-HMAC-SHA256","iterations":600000,"salt":"BASE64","cipher":"AES-256-GCM","nonce":"BASE64","ciphertext":"BASE64"}
```

Only the three BASE64 strings vary. Parsers require this canonical outer grammar; unknown, duplicate, reordered, escaped, missing, additional or changed parameters are rejected **before KDF**. This is deliberately a fixed envelope, not a general JSON metadata extension surface. Files larger than 6291456 bytes are rejected before parsing/reading fully. Minimum ciphertext is 17 decoded bytes (nonempty plaintext plus tag).

- Password: valid Unicode scalar sequence encoded to UTF-8, **no trim, normalization, case fold or URL decoding**. Reject unpaired UTF-16 surrogates on Java; UTF-8 length 8–1024 bytes. Confirmation is a UI concern. Password is never stored inside the envelope/payload/log/config.
- KDF: PBKDF2-HMAC-SHA256, exactly 600000 iterations, exactly 16 random salt bytes, derived key exactly 32 bytes. Use maintained platform/library implementations, not custom PBKDF2 code. Password bytes must match UTF-8; Java uses PBKDF2WithHmacSHA256 provider after Unicode vectors prove its encoding.
- AEAD: AES-256-GCM, exactly 12 random nonce bytes, exactly 16 tag bytes. `ciphertext` is ciphertext followed by tag, **not** separate tag or nonce prefix. Salt and nonce come from OS CSPRNG for every production export.
- AAD is exactly the following ASCII bytes, with no NUL, BOM, space or line ending: `muse-account-transfer:v1:PBKDF2-HMAC-SHA256:600000:AES-256-GCM`.
- Base64: RFC4648 standard alphabet `A–Z a–z 0–9 + /`, mandatory canonical `=` padding when needed, no whitespace or URL-safe alphabet. Salt encodes to 24 chars; nonce to 16. Decode and re-encode must match exactly.
- Salt/nonce/header tampering must fail validation/authentication. Fixed metadata participates via constant AAD plus strict envelope validation; salt derives key and nonce is the GCM nonce. Never decrypt or parse payload before GCM authentication completes.
- Plaintext is one valid UTF-8 JSON object, no BOM, 1–4194304 bytes, depth at most 16. Do not write plaintext to temporary files. Zero mutable password/derived key/plaintext buffers when no longer required.

Errors: `format` for noncanonical/unknown/oversized envelope or parameters; `password` for invalid password Unicode/length; `authentication` for wrong password or GCM authentication failure (indistinguishable to UI); `payload` for authenticated invalid logical data; `cancelled` for cancelled flow. Messages must never echo Cookie, password, proxy authentication, plaintext or native exception text containing them. Unsupported input must not trigger account/profile/proxy activation or network navigation.

## 2. Logical payload

UTF-8 JSON object, exact allowed root keys `format`, `version`, `accounts`, `proxies`; format=`muse-account-payload`, version=1. JSON object order/whitespace is irrelevant inside authenticated plaintext; no duplicate object keys or unknown fields. At most 32 accounts and 32 proxies; at least one account. IDs are package-local references matching `[A-Za-z0-9_-]{1,64}` and unique within their collection. No source local UUID/profile path/vault blob/revision/internal bridge appears on wire.

Proxy allowed keys: `proxy_ref`, `name`, `protocol`, `host`, `port`, `auth`.

- `name`: nonempty ≤128 Unicode scalars/512 UTF-8 bytes, no control chars. `protocol`: http/https/socks5. Host is a plain nonempty hostname/IP, no userinfo/path/query/fragment; IPv6 is stored without brackets. Port is integer 1–65535.
- `auth` is null or exactly `{ "username": STRING, "password": STRING }`. Values are opaque Unicode, unchanged; username nonempty, no colon/control chars, ≤1024 UTF-8 bytes; password ≤4096 UTF-8 bytes/no control chars. SOCKS5 username/password each ≤255 UTF-8 bytes. No supplier session parsing. No auth serializes as null.
- Only referenced proxies are allowed. Same proxy_ref is imported once and can be shared by multiple new accounts; do not merge different refs into existing local nodes by name/endpoint. Source local vault encryption is never portable.

Account allowed keys: `account_ref`, `name`, `network`, `cookies`.

- `name`: same name bounds. `network` exactly has `mode`, `proxy_ref`, `bypass_policy`, `source_force_proxy`.
- mode=`proxy`: proxy_ref must refer to an existing package proxy. bypass_policy=`strict` or `platform`; source_force_proxy is null/boolean provenance only. strict means the source actually removes implicit loopback bypass; platform means source adapter defaults/managed behavior, not direct fallback. mode=`system`: proxy_ref=null, bypass_policy=`platform`, source_force_proxy=null.
- On Android proxy import, target behavior is existing strict routing. Preview must explain a platform→strict conversion and obtain local confirmation; preserve source_force_proxy as provenance only. Re-export describes **actual target strict policy**, not source false as current execution. PC adapters declare their actual achievable policy. No new bypass management module. Any proxy restoration failure fails closed for Muse.
- System network and device-related addresses must be visibly confirmed in import preview; no automatic localhost/LAN rewriting. This carries configuration, not source device networking.

`cookies` exactly has `fidelity`, `coverage_urls`, `items`.

- fidelity=`attributes` or `header`; coverage_urls is 1–16 unique trusted URLs; items is 1–256 objects. This is URL-scoped data, never a complete-jar guarantee. Preserve duplicate names on different scopes; no name-only coalescing. Require nonempty hatch_sess for import, not hatch_vml/hatch_gw.
- Trusted source/coverage URLs use HTTPS, muse.ai or www.muse.ai, default port/443, no userinfo/fragment. Cookie domain is null or muse.ai/.muse.ai/www.muse.ai/.www.muse.ai and must be consistent with source_url; no unrelated origins.
- Cookie keys: `source_url`,`name`,`value`,`domain`,`path`,`expires_at_unix_seconds`,`secure`,`http_only`,`same_site`,`session`,`host_only`; all keys present. Nullable properties mean unknown, not false or never-expiring.
- name is HTTP token, 1–256 ASCII chars. Value ≤16384 UTF-8 bytes; no controls or semicolon, equals/percent/plus remain byte-for-byte unchanged. Path null or leading `/`, ≤1024 UTF-8 bytes, no controls/semicolon.
- expires_at_unix_seconds null or integer 1–253402300799; **seconds**, not milliseconds, never invented. secure/http_only/session/host_only null or boolean. same_site null/unspecified/lax/strict/none. session=true requires expiry=null; session=false requires expiry nonnull. Unknown session remains null. Expired essential login state is not made fresh by import; do not extend expiry.
- getCookieInfo may supply normalized domain/path/expires/flags; absence of explicit host-only metadata remains null. No assumption of all URLs or partition keys. Unsupported partitioned cookies must be clearly rejected rather than silently reinterpreted.
- Old header input is a separate compatibility UI, not an encrypted package: optional `Cookie:` prefix; split pairs at semicolons and only first equals; never URL decode. It becomes fidelity=header with null missing metadata. Preview explains target HTTPS host/root/session mapping and unknown HttpOnly/SameSite; no original expiry is invented.

## 3. Activation and limits

After full file authentication and schema/reference validation, generate new local account/proxy IDs. Create no schedulable account before isolated profile and explicit route preparation, all Cookie callback successes and flush. Then atomically save new candidate accounts and activate. Cancellation/crash/stale callbacks/failure must not publish partial accounts or touch unrelated/shared local nodes. Foreground, parked pages, quota and studio use the same route/activation barrier. Package display names are not authenticated server identity; actual website identity is a separate user acceptance check.

## 4. Interoperability evidence

`checks/fixtures/account-transfer-v1/` holds public synthetic plaintext, ASCII and Chinese/emoji test passwords, fixed salt/nonce, expected derived keys/envelopes and independent reference metadata. Deterministic salt/nonce overloads are test-only. Each real Java/Rust implementation must also generate a new random synthetic envelope consumed by the other and report file SHA-256 and producer/consumer implementation; matching fixed vectors alone is insufficient.
