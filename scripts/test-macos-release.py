#!/usr/bin/env python3
"""功能：验证两种签名的发布门禁，防止缺包、错版、篡改或签名模式错误；时间：2026-11-13；作者：lq。"""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("release", Path(__file__).with_name("prepare-macos-release.py"))
RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE)


class ReleaseTests(unittest.TestCase):
    """功能：使用隔离的双架构合成产物验证失败路径，无需 Apple 凭据；时间：2026-11-13；作者：lq。"""

    def setUp(self):
        """功能：生成完整的签名、公证和哈希记录；时间：2026-11-13；作者：lq。"""
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.output = self.root / "release"
        for arch, target in (("arm64", "aarch64-apple-darwin"), ("x86_64", "x86_64-apple-darwin")):
            package = self.root / f"macos-{arch}"
            package.mkdir()
            manifest = {"version": "0.1.0", "platform": target, "channel": "internal",
                        "baseline_commit": "abc123", "build_number": "1234", "working_tree_has_changes": False,
                        "signature": "Developer ID; hardened runtime; Apple notarization; stapled APP and DMG"}
            (package / "build-manifest.json").write_text(json.dumps(manifest))
            (package / "dynamic-libraries.txt").write_text("system libraries")
            for name in ("app-notarization.json", "dmg-notarization.json"):
                (package / name).write_text(json.dumps({"status": "Accepted", "id": "test-submission"}))
            hashes = []
            for ext in ("dmg", "zip"):
                name = f"QDS6_0.1.0_macOS_{arch}_internal.{ext}"
                payload = f"synthetic {arch} {ext}".encode()
                (package / name).write_bytes(payload)
                hashes.append(f"{hashlib.sha256(payload).hexdigest()}  {name}\n")
            (package / "SHA256SUMS").write_text("".join(hashes))

    def prepare(self, signing_mode="developer-id"):
        """功能：模拟 CI 指定的固定发布身份；时间：2026-11-13；作者：lq。"""
        RELEASE.prepare(self.root, self.output, "0.1.0", "abc123", "1234", signing_mode)

    def test_adhoc_without_apple_credentials_or_reports(self):
        """功能：无 Apple 公证报告的 ad-hoc 双架构包可发布且不声称已公证；时间：2026-11-13；作者：lq。"""
        for package in self.root.glob("macos-*"):
            path = package / "build-manifest.json"
            manifest = json.loads(path.read_text())
            manifest["signature"] = "ad-hoc; no Developer ID / notarization"
            path.write_text(json.dumps(manifest))
            for report in package.glob("*-notarization.json"):
                report.unlink()
        with self.assertRaisesRegex(ValueError, "signature"):
            self.prepare("developer-id")
        self.prepare("adhoc")
        self.assertEqual(len(list(self.output.glob("*.dmg"))), 2)
        self.assertFalse(list(self.output.glob("*-notarization.json")))
        notes = (self.output / "RELEASE-NOTES.md").read_text()
        self.assertIn("ad-hoc", notes)
        self.assertNotIn("已使用 Developer ID", notes)

    def test_signing_mode_mismatch(self):
        """功能：发布选项与实际包签名不一致时拒绝发布；时间：2026-11-13；作者：lq。"""
        with self.assertRaisesRegex(ValueError, "signature"):
            self.prepare("adhoc")

    def test_complete_release(self):
        """功能：双架构通过并输出四个可独立核验的安装包；时间：2026-11-13；作者：lq。"""
        self.prepare()
        self.assertEqual(len(list(self.output.glob("*.dmg"))), 2)
        self.assertEqual(len(list(self.output.glob("*.zip"))), 2)
        self.assertEqual(len((self.output / "SHA256SUMS").read_text().splitlines()), 4)
        self.assertEqual(len(list(self.output.glob("*-notarization.json"))), 4)

    def test_tampered_archive(self):
        """功能：哈希改变时拒绝发布且不创建输出；时间：2026-11-13；作者：lq。"""
        next((self.root / "macos-arm64").glob("*.dmg")).write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "哈希"):
            self.prepare()
        self.assertFalse(self.output.exists())

    def test_wrong_commit(self):
        """功能：拒绝来自其他提交的安装包；时间：2026-11-13；作者：lq。"""
        path = self.root / "macos-x86_64/build-manifest.json"
        manifest = json.loads(path.read_text())
        manifest["baseline_commit"] = "wrong"
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "baseline_commit"):
            self.prepare()

    def test_unnotarized(self):
        """功能：Apple 拒绝记录不得进入发布；时间：2026-11-13；作者：lq。"""
        (self.root / "macos-arm64/app-notarization.json").write_text('{"status":"Invalid","id":"test"}')
        with self.assertRaisesRegex(ValueError, "公证"):
            self.prepare()

    def test_missing_architecture(self):
        """功能：任意架构缺包均不得发布半套产物；时间：2026-11-13；作者：lq。"""
        (self.root / "macos-x86_64/build-manifest.json").unlink()
        with self.assertRaises(FileNotFoundError):
            self.prepare()
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
