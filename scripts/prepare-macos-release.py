#!/usr/bin/env python3
"""功能：校验双架构安装包身份和哈希，生成不重名的 Release 附件；时间：2026-11-13；作者：lq。"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil


def prepare(directory, output, version, commit, build_number, signing_mode="adhoc"):
    """功能：两架构全部验证通过后才整理附件，拒绝缺包、错版和被篡改包；时间：2026-11-13；作者：lq。"""
    # 功能：按用户选择严格核对签名模式，正式模式不得降级为 ad-hoc；时间：2026-11-13；作者：lq。
    signatures = {"adhoc": "ad-hoc; no Developer ID / notarization",
                  "developer-id": "Developer ID; hardened runtime; Apple notarization; stapled APP and DMG"}
    if signing_mode not in signatures:
        raise ValueError(f"未知签名方式：{signing_mode}")
    sources = []
    for architecture, target in (("arm64", "aarch64-apple-darwin"),
                                 ("x86_64", "x86_64-apple-darwin")):
        package = directory / f"macos-{architecture}"
        manifest = json.loads((package / "build-manifest.json").read_text())
        expected = {"version": version, "platform": target, "channel": "internal",
                    "baseline_commit": commit, "build_number": build_number,
                    "signature": signatures[signing_mode],
                    "working_tree_has_changes": False}
        for key, value in expected.items():
            if manifest.get(key) != value:
                raise ValueError(f"{architecture}: {key} 不匹配，预期 {value!r}")
        checksums = {}
        for line in (package / "SHA256SUMS").read_text().splitlines():
            digest, filename = line.split("  ", 1)
            if filename in checksums:
                raise ValueError(f"重复校验条目：{filename}")
            checksums[filename] = digest
        filenames = {f"QDS6_{version}_macOS_{architecture}_internal.{ext}" for ext in ("dmg", "zip")}
        if set(checksums) != filenames:
            raise ValueError(f"{architecture}: 必须包含且仅包含 DMG 和 ZIP 的校验条目")
        for filename in sorted(filenames):
            path = package / filename
            with path.open("rb") as source:
                digest = hashlib.file_digest(source, "sha256").hexdigest()
            if digest != checksums[filename]:
                raise ValueError(f"安装包哈希不匹配：{filename}")
            sources.append((path, filename, digest))
        notarization_files = ("app-notarization.json", "dmg-notarization.json") if signing_mode == "developer-id" else ()
        for filename in notarization_files:
            report = json.loads((package / filename).read_text())
            if report.get("status") != "Accepted" or not report.get("id"):
                raise ValueError(f"Apple 公证记录无效：{architecture}/{filename}")
        for filename in ("build-manifest.json", "dynamic-libraries.txt", *notarization_files):
            path = package / filename
            if not path.is_file():
                raise ValueError(f"缺少构建记录：{path}")
            sources.append((path, f"{architecture}-{filename}", None))
    output.mkdir(parents=True, exist_ok=False)
    for source, filename, _ in sources:
        shutil.copy2(source, output / filename)
    (output / "SHA256SUMS").write_text("".join(
        f"{digest}  {filename}\n" for _, filename, digest in sources if digest))
    # 功能：下载页明确区分测试签名和已公证签名，避免误报安装保障；时间：2026-11-13；作者：lq。
    signing_note = (
        "- 包含第三方许可，已使用 Developer ID 签名并通过 Apple 公证，APP 与 DMG 均已装订公证票据。\n"
        if signing_mode == "developer-id" else
        "- 包含第三方许可，使用 ad-hoc 本机测试签名，未经过 Apple 公证；首次打开可能被 macOS 拦截。\n"
        "- 请核对下载来源与校验值；如系统提供“仍要打开”，可在“系统设置 → 隐私与安全”中确认。\n"
    )
    (output / "RELEASE-NOTES.md").write_text(
        f"QDS6 {version} macOS 内测包\n\n"
        f"源码提交：{commit}\n构建号：{build_number}\n\n"
        "- Apple Silicon（M 系列）请选择 arm64；Intel Mac 请选择 x86_64。\n"
        "- 提供 DMG 和 ZIP；最低系统版本 macOS 14。\n"
        "- SHA256SUMS 提供四个安装包的 SHA-256，可用 shasum -a 256 -c SHA256SUMS 校验。\n"
        + signing_note +
        "- CI 编译、单测、签名、DMG 和哈希验证不代表真实服务、桌面交互及设备安装验收。\n"
        "- 退出旧应用后手动替换 QDS6.app，保留已有连接配置。\n")


def run():
    """功能：接收 CI 固定提交及构建身份，不接受不完整的发布集合；时间：2026-11-13；作者：lq。"""
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--build-number", required=True)
    parser.add_argument("--signing-mode", choices=["adhoc", "developer-id"], default="adhoc")
    options = parser.parse_args()
    prepare(options.directory, options.output, options.version, options.commit, options.build_number, options.signing_mode)


if __name__ == "__main__":
    run()
