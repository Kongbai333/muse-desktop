"""Generate public synthetic vectors using independent maintained crypto APIs.
Never use this script with user sessions or credentials.
"""
import base64
import hashlib
import json
from pathlib import Path
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

ROOT = Path(__file__).parent
AAD = b"muse-account-transfer:v1:PBKDF2-HMAC-SHA256:600000:AES-256-GCM"
payload = {
    "format": "muse-account-payload", "version": 1,
    "accounts": [{
        "account_ref": "account-a", "name": "合成账号 A",
        "network": {"mode": "proxy", "proxy_ref": "proxy-a", "bypass_policy": "strict", "source_force_proxy": None},
        "cookies": {"fidelity": "attributes", "coverage_urls": ["https://muse.ai/", "https://muse.ai/api/session"],
                    "items": [{"source_url": "https://muse.ai/api/session", "name": "hatch_sess",
                               "value": "SYNTHETIC_SESSION==%2F%3D%25+ONLY", "domain": ".muse.ai", "path": "/",
                               "expires_at_unix_seconds": 1893456000, "secure": True, "http_only": True,
                               "same_site": "lax", "session": False, "host_only": None}]}}],
    "proxies": [{"proxy_ref": "proxy-a", "name": "合成代理", "protocol": "http",
                 "host": "proxy.example.invalid", "port": 8080,
                 "auth": {"username": "SYNTHETIC_USER_raw+%3D=", "password": "SYNTHETIC_ONLY"}}],
}
plain = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
(ROOT / "payload.json").write_bytes(plain)
salt, nonce = bytes(range(16)), bytes(range(16, 28))
for label, phrase in [("ascii", "SYNTHETIC-test-pass-2026"), ("unicode", "合成互通🔐-SYNTHETIC-2026")]:
    encoded = phrase.encode("utf-8")
    key = hashlib.pbkdf2_hmac("sha256", encoded, salt, 600000, 32)
    ciphertext = AESGCM(key).encrypt(nonce, plain, AAD)
    b64 = lambda value: base64.b64encode(value).decode("ascii")
    envelope = ("{\"format\":\"muse-account-transfer\",\"version\":1,\"kdf\":\"PBKDF2-HMAC-SHA256\","
                "\"iterations\":600000,\"salt\":\"" + b64(salt) + "\",\"cipher\":\"AES-256-GCM\","
                "\"nonce\":\"" + b64(nonce) + "\",\"ciphertext\":\"" + b64(ciphertext) + "\"}").encode("ascii")
    (ROOT / (label + ".muse-account")).write_bytes(envelope)
    values = {"producer": "Python hashlib.pbkdf2_hmac and cryptography AESGCM (synthetic only)",
              "passphrase_utf8_base64": b64(encoded), "salt_hex": salt.hex(), "nonce_hex": nonce.hex(),
              "derived_key_hex": key.hex(), "payload_sha256": hashlib.sha256(plain).hexdigest(),
              "envelope_sha256": hashlib.sha256(envelope).hexdigest()}
    (ROOT / (label + ".properties")).write_text("\n".join(k + "=" + v for k, v in values.items()) + "\n", encoding="utf-8")
print("Generated two public synthetic reference vectors (ASCII and Chinese/emoji); no network.")
