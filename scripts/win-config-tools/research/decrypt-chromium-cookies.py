# -*- coding: utf-8 -*-
"""Try to decrypt commandcode.ai cookies from Edge (and other Chromium) profiles."""
import base64
import ctypes
import ctypes.wintypes as wt
import json
import shutil
import sqlite3
import tempfile
from pathlib import Path

CANDIDATES = [
    (Path(r"C:\Users\rwu3\AppData\Local\Microsoft\Edge\User Data"), "Edge"),
    (Path(r"C:\Users\rwu3\AppData\Local\Google\Chrome\User Data"), "Chrome"),
]


def dpapi_unprotect(data: bytes) -> bytes:
    class DATA_BLOB(ctypes.Structure):
        _fields_ = [("cbData", wt.DWORD), ("pbData", ctypes.POINTER(ctypes.c_char))]

    buf = ctypes.create_string_buffer(data, len(data))
    blob_in = DATA_BLOB(len(data), ctypes.cast(buf, ctypes.POINTER(ctypes.c_char)))
    blob_out = DATA_BLOB()
    if not ctypes.windll.crypt32.CryptUnprotectData(
        ctypes.byref(blob_in), None, None, None, None, 0, ctypes.byref(blob_out)
    ):
        raise OSError("CryptUnprotectData failed")
    try:
        return ctypes.string_at(blob_out.pbData, blob_out.cbData)
    finally:
        ctypes.windll.kernel32.LocalFree(blob_out.pbData)


def aes_gcm_decrypt(key: bytes, nonce: bytes, ciphertext: bytes) -> bytes:
    bcrypt = ctypes.windll.bcrypt
    AES = "AES".encode("utf-16-le")
    CM = "ChainingMode".encode("utf-16-le")
    GCM = "ChainingModeGCM".encode("utf-16-le")

    hAlg = ctypes.c_void_p()
    assert bcrypt.BCryptOpenAlgorithmProvider(ctypes.byref(hAlg), AES, None, 0) == 0
    try:
        assert bcrypt.BCryptSetProperty(hAlg, CM, GCM, len(GCM) + 2, 0) == 0
        hKey = ctypes.c_void_p()
        assert bcrypt.BCryptGenerateSymmetricKey(
            hAlg, ctypes.byref(hKey), None, 0, key, len(key), 0) == 0
        try:
            class AUTH(ctypes.Structure):
                _fields_ = [
                    ("pbNonce", ctypes.POINTER(ctypes.c_char)), ("cbNonce", ctypes.ULONG),
                    ("pbAuthData", ctypes.POINTER(ctypes.c_char)), ("cbAuthData", ctypes.ULONG),
                    ("pbTag", ctypes.POINTER(ctypes.c_char)), ("cbTag", ctypes.ULONG),
                    ("pbMacContext", ctypes.POINTER(ctypes.c_char)), ("cbMacContext", ctypes.ULONG),
                    ("cbAAD", ctypes.ULONG), ("cbData", ctypes.c_ulonglong), ("dwFlags", ctypes.ULONG),
                ]
            nonce_buf = ctypes.create_string_buffer(nonce, len(nonce))
            tag_buf = ctypes.create_string_buffer(ciphertext[-16:])
            ct = ciphertext[:-16]
            ct_buf = ctypes.create_string_buffer(ct, len(ct))
            pt_buf = ctypes.create_string_buffer(len(ct))
            auth = AUTH()
            auth.pbNonce = ctypes.cast(nonce_buf, ctypes.POINTER(ctypes.c_char))
            auth.cbNonce = len(nonce)
            auth.pbTag = ctypes.cast(tag_buf, ctypes.POINTER(ctypes.c_char))
            auth.cbTag = 16
            pt_len = ctypes.c_ulong()
            st = bcrypt.BCryptDecrypt(hKey, ct_buf, len(ct), ctypes.byref(auth), None, 0,
                                      pt_buf, len(ct), ctypes.byref(pt_len), 0)
            assert st == 0, f"decrypt {st:#x}"
            return pt_buf.raw[: pt_len.value]
        finally:
            bcrypt.BCryptDestroyKey(hKey)
    finally:
        bcrypt.BCryptCloseAlgorithmProvider(hAlg, 0)


def try_profile(user_data: Path, label: str):
    profiles = [p for p in user_data.glob("*/Network/Cookies") if p.parent.parent.name in
                ("Default",) or (p.parent.parent / "Preferences").exists()]
    if not profiles:
        print(f"[{label}] no cookie DBs found")
        return
    local_state = user_data / "Local State"
    if not local_state.exists():
        print(f"[{label}] no Local State")
        return
    ls = json.loads(local_state.read_text(encoding="utf-8"))
    enc_key = base64.b64decode(ls["os_crypt"]["encrypted_key"])
    aes_key = dpapi_unprotect(enc_key[5:])

    for cookies_db in profiles:
        pname = cookies_db.parent.parent.name
        try:
            tmp = Path(tempfile.mkdtemp(prefix="ck_"))
            db = tmp / "Cookies"
            shutil.copy2(cookies_db, db)
        except PermissionError:
            print(f"[{label}/{pname}] locked (browser running) - skipped")
            continue
        for ext in ("-wal", "-shm", "-journal"):
            src = Path(str(cookies_db) + ext)
            if src.exists():
                try:
                    shutil.copy2(src, str(db) + ext)
                except PermissionError:
                    pass
        con = sqlite3.connect(str(db))
        rows = con.execute(
            "SELECT host_key, name, encrypted_value, is_httponly FROM cookies "
            "WHERE host_key LIKE '%commandcode%'").fetchall()
        con.close()
        print(f"[{label}/{pname}] commandcode cookies: {len(rows)}")
        for host, name, ev, httponly in rows:
            try:
                if ev[:3] in (b"v10", b"v20"):
                    val = aes_gcm_decrypt(aes_key, ev[3:15], ev[15:]).decode("utf-8", "replace")
                else:
                    val = dpapi_unprotect(ev).decode("utf-8", "replace")
                print(f"  {host}  {name}  httponly={bool(httponly)}  len={len(val)}  {val[:24]}...")
                if "session" in name or "better-auth" in name:
                    out = Path(f"cookie_{label}_{pname}.json")
                    out.write_text(json.dumps({f"{host}|{name}": val}), encoding="utf-8")
                    print(f"    -> saved to {out.name}")
            except Exception as e:
                print(f"  {host}  {name}  DECRYPT FAILED: {e}")
        if not rows:
            # show what hosts exist for sanity
            con = sqlite3.connect(str(db))
            n = con.execute("SELECT COUNT(*) FROM cookies").fetchone()[0]
            con.close()
            print(f"  (db total cookies: {n})")


for ud, label in CANDIDATES:
    if ud.exists():
        try:
            try_profile(ud, label)
        except Exception as e:
            print(f"[{label}] ERROR: {e}")
    else:
        print(f"[{label}] not installed")
