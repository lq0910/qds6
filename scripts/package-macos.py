#!/usr/bin/env python3
"""功能：生成无需 Homebrew 的原生 Mac 内部测试包并校验签名；时间：2026-11-11；作者：lq。"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def command(arguments):
    """功能：使用参数数组执行打包工具，失败立即停止；时间：2026-11-11；作者：lq。"""
    return subprocess.check_output(arguments, cwd=ROOT, stderr=subprocess.STDOUT).decode()


def sha256(path):
    """功能：发布产物内容校验；时间：2026-11-11；作者：lq。"""
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def run():
    """功能：创建 APP、ZIP、只读 DMG，保留原始许可和精确构建指纹；时间：2026-11-11；作者：lq。"""
    parser = argparse.ArgumentParser()
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--output", type=Path)
    options = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise SystemExit("当前脚本仅打包已验收的 Apple Silicon macOS 构建")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    output = options.output or ROOT / "output/release" / f"{version}-macos-arm64-internal"
    if output.exists():
        raise SystemExit("输出目录已存在；选择新路径，避免覆盖已验收包")
    if not options.skip_build:
        print(command(["cargo", "build", "-p", "qds6-app", "--release", "--locked"]))
    binary = ROOT / "target/release/qds6"
    dependencies = command(["otool", "-L", str(binary)])
    for line in dependencies.splitlines()[1:]:
        path = line.strip().split(" (", 1)[0]
        if not path.startswith(("/usr/lib/", "/System/Library/")):
            raise SystemExit(f"发行包仍有非系统动态依赖：{path}")
    notices = ROOT / "output/third-party"
    if not (notices / "dependencies.json").exists():
        raise SystemExit("请先运行 scripts/collect-licenses.py --fetch-missing")
    output.mkdir(parents=True)
    stage = output / "staging"
    app = stage / "QDS6.app"
    executable = app / "Contents/MacOS/qds6"
    resources = app / "Contents/Resources"
    executable.parent.mkdir(parents=True)
    resources.mkdir()
    shutil.copy2(binary, executable)
    executable.chmod(0o755)
    shutil.copy2(ROOT / "assets/brand/qds6.icns", resources / "qds6.icns")
    shutil.copy2(ROOT / "LICENSE", resources / "LICENSE")
    shutil.copytree(notices, resources / "ThirdParty")
    shutil.copy2(ROOT / "docs/macos-first-release/使用说明.md", resources / "使用说明.md")
    with (app / "Contents/Info.plist").open("wb") as target:
        plistlib.dump({
            "CFBundleName": "QDS6", "CFBundleDisplayName": "QDS6", "CFBundleExecutable": "qds6",
            "CFBundleIdentifier": "local.qds6.desktop", "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": version, "CFBundleVersion": "2026111101",
            "CFBundleIconFile": "qds6.icns", "LSMinimumSystemVersion": "14.0",
            "NSHighResolutionCapable": True, "NSPrincipalClass": "NSApplication",
            "LSApplicationCategoryType": "public.app-category.developer-tools",
            "CFBundleDevelopmentRegion": "zh_CN",
            "CFBundleLocalizations": ["zh_CN", "en"],
            "CFBundleGetInfoString": f"QDS6 {version} — 内部测试版 / Internal build; ad-hoc signed",
            "NSHumanReadableCopyright": "Copyright © 2026 lq. Third-party notices included.",
        }, target, sort_keys=True)
    files = command(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"]).split("\0")
    source_hashes = {p: sha256(ROOT / p) for p in sorted(set(files)) if p and (ROOT / p).is_file() and (p.startswith(("crates/", "assets/", ".cargo/")) or p in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml"))}
    manifest = {
        "description": "功能：未提交工作树与发行构建内容指纹；时间：2026-11-11；作者：lq。",
        "version": version, "channel": "internal", "platform": "aarch64-apple-darwin",
        "baseline_commit": command(["git", "rev-parse", "HEAD"]).strip(),
        "working_tree_has_changes": bool(command(["git", "status", "--porcelain"]).strip()),
        "cargo_lock_sha256": sha256(ROOT / "Cargo.lock"),
        "binary_sha256_before_bundle_signing": sha256(binary),
        "build": "cargo build -p qds6-app --release --locked (no test features)",
        "rust": command(["rustc", "-Vv"]), "macos": command(["sw_vers"]),
        "signature": "ad-hoc; no Developer ID / notarization", "source_sha256": source_hashes,
    }
    (resources / "build-manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    print(command(["codesign", "--force", "--sign", "-", "--timestamp=none", "--identifier", "local.qds6.desktop", str(app)]))
    print(command(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)]))
    shutil.copy2(ROOT / "docs/macos-first-release/使用说明.md", stage / "安装与使用说明.md")
    (stage / "Applications").symlink_to("/Applications", target_is_directory=True)
    name = f"QDS6_{version}_macOS_arm64_internal"
    zip_file = output / (name + ".zip")
    dmg_file = output / (name + ".dmg")
    print(command(["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(zip_file)]))
    print(command(["hdiutil", "create", "-volname", "QDS6 内部测试版", "-srcfolder", str(stage), "-format", "UDZO", str(dmg_file)]))
    print(command(["hdiutil", "verify", str(dmg_file)]))
    (output / "SHA256SUMS").write_text("\n".join(f"{sha256(path)}  {path.name}" for path in (zip_file, dmg_file)) + "\n")
    (output / "dynamic-libraries.txt").write_text(dependencies)
    shutil.copy2(resources / "build-manifest.json", output / "build-manifest.json")
    print(f"Created internal macOS package: {output}")


if __name__ == "__main__":
    run()
