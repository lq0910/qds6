#!/usr/bin/env python3
"""功能：内测版固定位置覆盖安装，只保留一份日常应用，失败恢复旧应用；时间：2026-11-11；作者：lq。"""
import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def command(arguments):
    """功能：不经过 shell 执行签名和复制工具；时间：2026-11-11；作者：lq。"""
    return subprocess.check_output(arguments, stderr=subprocess.STDOUT, text=True)


def digest(path):
    """功能：核对安装后二进制一致性，不访问用户配置和凭据；时间：2026-11-11；作者：lq。"""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run():
    """功能：从已验收的固定内测包覆盖安装，不按构建号新增 APP；时间：2026-11-11；作者：lq。"""
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    parser.add_argument("--target", type=Path, default=Path.home() / "Applications/QDS6.app")
    options = parser.parse_args()
    output = options.directory.resolve()
    target = options.target.expanduser().absolute()
    source = output / "staging/QDS6.app"
    manifest = json.loads((output / "build-manifest.json").read_text())
    verification = json.loads((output / "installation-verification.json").read_text())
    if manifest.get("channel") != "internal" or verification.get("fresh_profile_count") != 0:
        raise SystemExit("仅安装已完成隔离验收的内部包")
    if target.name != "QDS6.app" or target.is_symlink():
        raise SystemExit("安装目标必须是固定的 QDS6.app 目录")
    for app in [source] + ([target] if target.exists() else []):
        info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
        if info.get("CFBundleIdentifier") != "local.qds6.desktop":
            raise SystemExit("拒绝替换非 QDS6 应用")
        command(["codesign", "--verify", "--deep", "--strict", str(app)])
    processes = subprocess.run(["pgrep", "-f", str(target / "Contents/MacOS/qds6")], capture_output=True)
    if processes.returncode != 1:
        raise SystemExit("请先正常退出已安装的 QDS6，保留未保存内容")
    target.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".qds6-install-", dir=target.parent) as temporary:
        candidate = Path(temporary) / "QDS6.app"
        previous = Path(temporary) / "previous.app"
        command(["ditto", str(source), str(candidate)])
        command(["codesign", "--verify", "--deep", "--strict", str(candidate)])
        old_exists = target.exists()
        if old_exists:
            target.rename(previous)
        try:
            candidate.rename(target)
            command(["codesign", "--verify", "--deep", "--strict", str(target)])
            if digest(target / "Contents/MacOS/qds6") != digest(source / "Contents/MacOS/qds6"):
                raise RuntimeError("安装后二进制校验不一致")
        except BaseException:
            if target.exists():
                shutil.rmtree(target)
            if old_exists:
                previous.rename(target)
            raise
    # 功能：安装完成后移除工作区暂存 APP，避免启动器枚举重复版本；时间：2026-11-11；作者：lq。
    shutil.rmtree(source)
    report = {"target": str(target), "build_number": manifest["build_number"], "binary_sha256": digest(target / "Contents/MacOS/qds6"), "signature": "verified", "staging_app_removed": True}
    (output / "installed-app.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    run()
