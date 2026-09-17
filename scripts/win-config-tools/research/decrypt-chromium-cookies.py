# -*- coding: utf-8 -*-
"""Inspect commandcode.ai cookies from local Chromium profiles.

The default output contains only counts, lengths, and fingerprints. Use
``--export-dir`` explicitly when a plaintext cookie export is required.
"""

import argparse
import base64
import ctypes
import ctypes.wintypes as wt
import hashlib
import json
import os
import re
import shutil
import sqlite3
import tempfile
from pathlib import Path
from typing import Optional


def default_candidates():
    local_app_data = Path(
        os.environ.get("LOCALAPPDATA", Path.home() / "AppData" / "Local")
    )
    return [
        (local_app_data / "Microsoft" / "Edge" / "User Data", "Edge"),
        (local_app_data / "Google" / "Chrome" / "User Data", "Chrome"),
    ]


def check_bcrypt(status: int, operation: str) -> None:
    if status != 0:
        raise OSError(f"{operation} failed: 0x{status:08x}")


def dpapi_unprotect(data: bytes) -> bytes:
    if not data:
        raise ValueError("cannot decrypt an empty DPAPI value")

    class DataBlob(ctypes.Structure):
        _fields_ = [("cbData", wt.DWORD), ("pbData", ctypes.POINTER(ctypes.c_char))]

    buf = ctypes.create_string_buffer(data, len(data))
    blob_in = DataBlob(len(data), ctypes.cast(buf, ctypes.POINTER(ctypes.c_char)))
    blob_out = DataBlob()
    if not ctypes.windll.crypt32.CryptUnprotectData(
        ctypes.byref(blob_in), None, None, None, None, 0, ctypes.byref(blob_out)
    ):
        raise OSError("CryptUnprotectData failed")
    try:
        return ctypes.string_at(blob_out.pbData, blob_out.cbData)
    finally:
        ctypes.windll.kernel32.LocalFree(blob_out.pbData)


def aes_gcm_decrypt(key: bytes, nonce: bytes, ciphertext: bytes) -> bytes:
    if len(ciphertext) < 16:
        raise ValueError("AES-GCM value is missing its authentication tag")

    bcrypt = ctypes.windll.bcrypt
    aes = "AES".encode("utf-16-le")
    chaining_mode = "ChainingMode".encode("utf-16-le")
    gcm = "ChainingModeGCM".encode("utf-16-le")

    algorithm = ctypes.c_void_p()
    check_bcrypt(
        bcrypt.BCryptOpenAlgorithmProvider(ctypes.byref(algorithm), aes, None, 0),
        "BCryptOpenAlgorithmProvider",
    )
    try:
        check_bcrypt(
            bcrypt.BCryptSetProperty(
                algorithm, chaining_mode, gcm, len(gcm) + 2, 0
            ),
            "BCryptSetProperty",
        )
        key_handle = ctypes.c_void_p()
        check_bcrypt(
            bcrypt.BCryptGenerateSymmetricKey(
                algorithm, ctypes.byref(key_handle), None, 0, key, len(key), 0
            ),
            "BCryptGenerateSymmetricKey",
        )
        try:
            class AuthInfo(ctypes.Structure):
                _fields_ = [
                    ("pbNonce", ctypes.POINTER(ctypes.c_char)),
                    ("cbNonce", ctypes.ULONG),
                    ("pbAuthData", ctypes.POINTER(ctypes.c_char)),
                    ("cbAuthData", ctypes.ULONG),
                    ("pbTag", ctypes.POINTER(ctypes.c_char)),
                    ("cbTag", ctypes.ULONG),
                    ("pbMacContext", ctypes.POINTER(ctypes.c_char)),
                    ("cbMacContext", ctypes.ULONG),
                    ("cbAAD", ctypes.ULONG),
                    ("cbData", ctypes.c_ulonglong),
                    ("dwFlags", ctypes.ULONG),
                ]

            nonce_buf = ctypes.create_string_buffer(nonce, len(nonce))
            tag_buf = ctypes.create_string_buffer(ciphertext[-16:])
            encrypted = ciphertext[:-16]
            encrypted_buf = ctypes.create_string_buffer(encrypted, len(encrypted))
            plain_buf = ctypes.create_string_buffer(len(encrypted))
            auth = AuthInfo()
            auth.pbNonce = ctypes.cast(nonce_buf, ctypes.POINTER(ctypes.c_char))
            auth.cbNonce = len(nonce)
            auth.pbTag = ctypes.cast(tag_buf, ctypes.POINTER(ctypes.c_char))
            auth.cbTag = 16
            plain_len = ctypes.c_ulong()
            check_bcrypt(
                bcrypt.BCryptDecrypt(
                    key_handle,
                    encrypted_buf,
                    len(encrypted),
                    ctypes.byref(auth),
                    None,
                    0,
                    plain_buf,
                    len(encrypted),
                    ctypes.byref(plain_len),
                    0,
                ),
                "BCryptDecrypt",
            )
            return plain_buf.raw[: plain_len.value]
        finally:
            bcrypt.BCryptDestroyKey(key_handle)
    finally:
        bcrypt.BCryptCloseAlgorithmProvider(algorithm, 0)


def profile_paths(user_data: Path):
    return [
        path
        for path in user_data.glob("*/Network/Cookies")
        if path.parent.parent.name == "Default"
        or (path.parent.parent / "Preferences").exists()
    ]


def fingerprint(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8", "replace")).hexdigest()[:12]


def safe_filename(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "_", value)


def export_cookie(
    export_dir: Path,
    label: str,
    profile: str,
    host: str,
    name: str,
    value: str,
) -> None:
    export_dir.mkdir(parents=True, exist_ok=True)
    filename = safe_filename(f"cookie_{label}_{profile}_{host}_{name}.json")
    output = export_dir / filename
    output.write_text(
        json.dumps({f"{host}|{name}": value}, ensure_ascii=False),
        encoding="utf-8",
    )
    print(f"    exported plaintext cookie to {output}")


def try_profile(user_data: Path, label: str, export_dir: Optional[Path]) -> None:
    profiles = profile_paths(user_data)
    if not profiles:
        print(f"[{label}] no cookie DBs found")
        return
    local_state = user_data / "Local State"
    if not local_state.exists():
        print(f"[{label}] no Local State")
        return
    local_state_json = json.loads(local_state.read_text(encoding="utf-8"))
    encrypted_key = base64.b64decode(local_state_json["os_crypt"]["encrypted_key"])
    aes_key = dpapi_unprotect(encrypted_key[5:])

    for cookies_db in profiles:
        profile = cookies_db.parent.parent.name
        try:
            with tempfile.TemporaryDirectory(prefix="codexbar-cookie-") as temp_dir:
                database = Path(temp_dir) / "Cookies"
                shutil.copy2(cookies_db, database)
                for extension in ("-wal", "-shm", "-journal"):
                    sidecar = Path(str(cookies_db) + extension)
                    if sidecar.exists():
                        try:
                            shutil.copy2(sidecar, str(database) + extension)
                        except PermissionError:
                            print(f"[{label}/{profile}] {extension} sidecar locked - skipped")

                with sqlite3.connect(database) as connection:
                    rows = connection.execute(
                        "SELECT host_key, name, encrypted_value, is_httponly "
                        "FROM cookies WHERE host_key LIKE '%commandcode%'"
                    ).fetchall()
                    total = (
                        connection.execute("SELECT COUNT(*) FROM cookies").fetchone()[0]
                        if not rows
                        else None
                    )

                print(f"[{label}/{profile}] commandcode cookies: {len(rows)}")
                for host, name, encrypted_value, httponly in rows:
                    try:
                        if encrypted_value[:3] in (b"v10", b"v20"):
                            value = aes_gcm_decrypt(
                                aes_key,
                                encrypted_value[3:15],
                                encrypted_value[15:],
                            ).decode("utf-8", "replace")
                        else:
                            value = dpapi_unprotect(encrypted_value).decode(
                                "utf-8", "replace"
                            )
                        print(
                            f"  {host}  {name}  httponly={bool(httponly)} "
                            f"len={len(value)} sha256={fingerprint(value)}"
                        )
                        if export_dir is not None and (
                            "session" in name.lower() or "better-auth" in name.lower()
                        ):
                            export_cookie(export_dir, label, profile, host, name, value)
                    except Exception as error:
                        print(f"  {host}  {name}  DECRYPT FAILED: {error}")
                if not rows:
                    print(f"  (db total cookies: {total})")
        except PermissionError:
            print(f"[{label}/{profile}] locked (browser running) - skipped")


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--user-data-dir",
        action="append",
        type=Path,
        dest="user_data_dirs",
        help="Chromium user-data directory; repeat for multiple profiles",
    )
    parser.add_argument(
        "--export-dir",
        type=Path,
        help="explicitly export decrypted session cookies to this directory",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    candidates = (
        [(path, path.name or "Browser") for path in args.user_data_dirs]
        if args.user_data_dirs
        else default_candidates()
    )
    for user_data, label in candidates:
        if user_data.exists():
            try:
                try_profile(user_data, label, args.export_dir)
            except Exception as error:
                print(f"[{label}] ERROR: {error}")
        else:
            print(f"[{label}] not installed")


if __name__ == "__main__":
    main()
