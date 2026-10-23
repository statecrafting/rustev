"""Generate synthetic offline Sigstore trust and a signed plan registration.

Usage: python3 regenerate_sigstore.py SIGNED_REGISTER_JSON OUTPUT_DIR
Or: python3 regenerate_sigstore.py REGISTER_JSON SIGNING_INPUT OUTPUT_DIR
Requires OpenSSL. Keys exist only in a temporary directory and are discarded.
No production roots, services, identities or network calls are used.
"""
import base64
import copy
import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import time


def b64(data):
    return base64.b64encode(data).decode()


def sha(data):
    return hashlib.sha256(data).digest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def der(tag, body):
    n = len(body)
    length = bytes([n]) if n < 128 else bytes([0x80 + (n.bit_length() + 7) // 8]) + n.to_bytes((n.bit_length() + 7) // 8, "big")
    return bytes([tag]) + length + body


def children(data):
    offset = 0
    result = []
    while offset < len(data):
        start = offset
        tag, length = data[offset:offset + 2]
        offset += 2
        if length & 0x80:
            count = length & 0x7f
            length = int.from_bytes(data[offset:offset + count], "big")
            offset += count
        body = data[offset:offset + length]
        offset += length
        result.append((tag, body, data[start:offset]))
    return result


def main():
    registration = json.loads(pathlib.Path(sys.argv[1]).read_text())
    if len(sys.argv) == 3:
        signature = registration["signature"]["some"]
        artifact = (signature["schema"] + "\n" + signature["bundle_digest"]).encode()
        output = pathlib.Path(sys.argv[2]).resolve()
    else:
        artifact = pathlib.Path(sys.argv[2]).read_bytes()
        output = pathlib.Path(sys.argv[3]).resolve()
    with tempfile.TemporaryDirectory(prefix="rustev-sigstore-") as tmp:
        work = pathlib.Path(tmp)

        def openssl(*args, data=None):
            return subprocess.run(["openssl", *args], cwd=work, input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True).stdout

        def sign(key, data):
            return openssl("dgst", "-sha256", "-sign", key + ".key", data=data)

        public = {}
        for key in ["root", "leaf", "ct", "rekor"]:
            openssl("genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256", "-out", key + ".key")
            public[key] = openssl("pkey", "-in", key + ".key", "-pubout", "-outform", "DER")
        openssl("req", "-new", "-x509", "-key", "root.key", "-subj", "/CN=Synthetic Rustev Fulcio", "-days", "3650", "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign", "-out", "root.pem")
        openssl("req", "-new", "-key", "leaf.key", "-subj", "/CN=Synthetic Rustev signer", "-out", "leaf.csr")
        (work / "leaf.ext").write_text("basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=codeSigning\nsubjectAltName=email:rustev@example.invalid\n1.3.6.1.4.1.57264.1.8=ASN1:UTF8String:https://issuer.example.invalid\n")
        openssl("x509", "-req", "-in", "leaf.csr", "-CA", "root.pem", "-CAkey", "root.key", "-set_serial", "42", "-days", "3650", "-extfile", "leaf.ext", "-outform", "DER", "-out", "leaf.der")
        certificate = (work / "leaf.der").read_bytes()
        cert_parts = children(children(certificate)[0][1])
        tbs = cert_parts[0][2]
        now = int(time.time())
        timestamp = (now * 1000).to_bytes(8, "big")
        signed_sct = b"\x00\x00" + timestamp + b"\x00\x01" + sha(public["root"]) + len(tbs).to_bytes(3, "big") + tbs + b"\x00\x00"
        sig = sign("ct", signed_sct)
        sct = b"\x00" + sha(public["ct"]) + timestamp + b"\x00\x00\x04\x03" + len(sig).to_bytes(2, "big") + sig
        entry = len(sct).to_bytes(2, "big") + sct
        sct_list = len(entry).to_bytes(2, "big") + entry
        # Embedded SCT extension: an OCTET STRING holding the TLS list.
        extension = der(0x30, bytes.fromhex("060a2b06010401d679020402") + der(4, der(4, sct_list)))
        fields = children(cert_parts[0][1])
        tag, body, _ = fields[-1]
        assert tag == 0xa3
        extensions = children(body)[0][1]
        new_tbs = der(0x30, b"".join(f[2] for f in fields[:-1]) + der(0xa3, der(0x30, extensions + extension)))
        certificate = der(0x30, new_tbs + cert_parts[1][2] + der(3, b"\x00" + sign("root", new_tbs)))
        pem = b"-----BEGIN CERTIFICATE-----\n" + base64.encodebytes(certificate) + b"-----END CERTIFICATE-----\n"
        signature = sign("leaf", artifact)
        log_body = canonical({"apiVersion": "0.0.1", "kind": "hashedrekord", "spec": {"data": {"hash": {"algorithm": "sha256", "value": sha(artifact).hex()}}, "signature": {"content": b64(signature), "publicKey": {"content": b64(pem)}}}})
        root_hash = sha(b"\x00" + log_body)
        log_id = sha(public["rekor"])
        checkpoint = f"synthetic.rekor.example.invalid\n1\n{b64(root_hash)}\n"
        checkpoint += "\n" + chr(0x2014) + " synthetic.rekor.example.invalid " + b64(log_id[:4] + sign("rekor", checkpoint.encode())) + "\n"
        promise = canonical({"body": b64(log_body), "integratedTime": now, "logIndex": 0, "logID": log_id.hex()})
        bundle = {"mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json", "verificationMaterial": {"certificate": {"rawBytes": b64(certificate)}, "tlogEntries": [{"logIndex": "0", "logId": {"keyId": b64(log_id)}, "kindVersion": {"kind": "hashedrekord", "version": "0.0.1"}, "integratedTime": str(now), "inclusionPromise": {"signedEntryTimestamp": b64(sign("rekor", promise))}, "inclusionProof": {"logIndex": "0", "rootHash": b64(root_hash), "treeSize": "1", "hashes": [], "checkpoint": {"envelope": checkpoint}}, "canonicalizedBody": b64(log_body)}]}, "messageSignature": {"messageDigest": {"algorithm": "SHA2_256", "digest": b64(sha(artifact))}, "signature": b64(signature)}}

        def log(key):
            return {"baseUrl": f"https://synthetic.{key}.example.invalid", "hashAlgorithm": "SHA2_256", "publicKey": {"rawBytes": b64(public[key]), "keyDetails": "PKIX_ECDSA_P256_SHA_256", "validFor": {"start": "2020-01-01T00:00:00Z"}}, "logId": {"keyId": b64(sha(public[key]))}}

        root = {"mediaType": "application/vnd.dev.sigstore.trustedroot+json;version=0.1", "tlogs": [log("rekor")], "ctlogs": [log("ct")], "certificateAuthorities": [{"subject": {"commonName": "Synthetic Rustev Fulcio"}, "uri": "https://synthetic.fulcio.example.invalid", "certChain": {"certificates": [{"rawBytes": b64(openssl("x509", "-in", "root.pem", "-outform", "DER"))}]}, "validFor": {"start": "2020-01-01T00:00:00Z"}}]}
        registration["signature"] = {"schema": "rustev.plan-bundle-signature/1", "bundle_digest": artifact.decode().split("\n")[1], "scheme": {"sigstore": {"bundle": bundle}}}
        registration["signature"] = {"some": registration["signature"]}
        # Same issuer name but a different CA key: the leaf chain must fail,
        # while the artifact signature and transparency proof remain intact.
        openssl("req", "-new", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes", "-keyout", "wrong-root.key", "-subj", "/CN=Synthetic Rustev Fulcio", "-days", "3650", "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign", "-out", "wrong-root.pem")
        wrong_root = copy.deepcopy(root)
        wrong_root["certificateAuthorities"][0]["certChain"]["certificates"][0]["rawBytes"] = b64(openssl("x509", "-in", "wrong-root.pem", "-outform", "DER"))
        (output / "synthetic-sigstore-invalid-chain-root.json").write_text(json.dumps(wrong_root, indent=2) + "\n")
        (output / "synthetic-sigstore-root.json").write_text(json.dumps(root, indent=2) + "\n")
        (output / "synthetic-sigstore-register.json").write_text(json.dumps(registration, indent=2) + "\n")


if __name__ == "__main__":
    main()
