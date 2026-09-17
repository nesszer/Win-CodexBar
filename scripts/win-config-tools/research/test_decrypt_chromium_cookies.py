import importlib.util
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("decrypt-chromium-cookies.py")
SPEC = importlib.util.spec_from_file_location("decrypt_chromium_cookies", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise ImportError(f"unable to load {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SessionCookieNameTests(unittest.TestCase):
    def test_recognizes_canonical_and_casing_variants_of_all_names(self):
        for name in sorted(MODULE.SESSION_COOKIE_NAMES):
            with self.subTest(name=name):
                self.assertTrue(MODULE.is_session_cookie_name(name))
                self.assertTrue(MODULE.is_session_cookie_name(name.swapcase()))

    def test_rejects_unrelated_near_miss(self):
        self.assertFalse(
            MODULE.is_session_cookie_name("__Secure-commandcode_prod_.session_token.evil")
        )


if __name__ == "__main__":
    unittest.main()
