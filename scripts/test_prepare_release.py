import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("prepare_release", Path(__file__).with_name("prepare-release.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.parts = self.root / 'parts'
        self.parts.mkdir()
        self.android = self.root / 'android'
        self.android.mkdir()
        self.out = self.root / 'out'
        for name in ['app-setup.exe', 'app.dmg', 'app.app.tar.gz', 'app.deb', 'app.AppImage']:
            (self.parts / name).write_bytes(b'installer')
            if name != 'app.dmg':
                (self.parts / (name + '.sig')).write_text('encoded signature')
        (self.android / 'PrivateMusic_0.6.6_android-arm64.apk').write_bytes(b'apk')

    def tearDown(self):
        self.tmp.cleanup()

    def run_prepare(self):
        module.prepare(self.parts, self.android, self.out, '0.6.6', 'decoder-dev/PrivateMusicDesktop')

    def test_complete_release_selects_correct_bundle_for_each_installation(self):
        self.run_prepare()
        manifest = json.loads((self.out / 'latest.json').read_text())
        targets = manifest['platforms']
        self.assertEqual(len(targets), 5)
        self.assertTrue(targets['linux-x86_64-deb']['url'].endswith('.deb'))
        self.assertTrue(targets['linux-x86_64-appimage']['url'].endswith('.AppImage'))
        self.assertEqual(targets['darwin-x86_64-app'], targets['darwin-aarch64-app'])
        self.assertTrue(targets['windows-x86_64-nsis']['url'].endswith('-setup.exe'))
        import hashlib
        for line in (self.out / 'SHA256SUMS.txt').read_text().splitlines():
            digest, name = line.split('  ')
            self.assertEqual(hashlib.sha256((self.out / name).read_bytes()).hexdigest(), digest)

    def test_missing_signature_blocks_the_entire_release(self):
        (self.parts / 'app.deb.sig').unlink()
        with self.assertRaises(ValueError): self.run_prepare()
        self.assertFalse(self.out.exists())

    def test_duplicate_payload_blocks_the_entire_release(self):
        (self.parts / 'other.deb').write_bytes(b'other')
        with self.assertRaises(ValueError): self.run_prepare()
        self.assertFalse(self.out.exists())

    def test_unsigned_android_is_not_published_as_installable(self):
        apk = next(self.android.iterdir())
        apk.rename(apk.with_name('PrivateMusic_0.6.6_android-arm64-unsigned.apk'))
        with self.assertRaises(ValueError): self.run_prepare()
        self.assertFalse(self.out.exists())

if __name__ == '__main__': unittest.main()
