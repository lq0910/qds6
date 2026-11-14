#!/usr/bin/env python3
"""功能：CI 临时钥匙串导入签名和公证凭据，结束时清理；时间：2026-11-13；作者：lq。"""
import argparse
import base64
import json
import os
from pathlib import Path
import secrets
import shlex
import shutil
import subprocess


def command(arguments):
    """功能：不回显含密码的参数，失败只报告工具名称；时间：2026-11-13；作者：lq。"""
    result = subprocess.run(arguments, capture_output=True, text=True)
    if result.returncode:
        raise SystemExit(f"{arguments[0]} {arguments[1]} 失败，请检查证书、密码和公证 API 权限")
    return result.stdout


def run():
    """功能：仅从 Actions Secrets 环境读取凭据，私钥不进入源码和产物；时间：2026-11-13；作者：lq。"""
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["check", "setup", "cleanup"])
    options = parser.parse_args()
    directory = Path(os.environ["RUNNER_TEMP"]) / "qds6-signing"
    keychain = directory / "release.keychain-db"
    search_list = directory / "previous-keychains.json"
    if options.action == "cleanup":
        if search_list.exists():
            command(["security", "list-keychains", "-d", "user", "-s", *json.loads(search_list.read_text())])
        if keychain.exists():
            command(["security", "delete-keychain", str(keychain)])
        shutil.rmtree(directory, ignore_errors=True)
        return
    required = ["MACOS_CERTIFICATE_P12_BASE64", "MACOS_CERTIFICATE_PASSWORD", "MACOS_SIGNING_IDENTITY",
                "APPLE_API_KEY_P8", "APPLE_API_KEY_ID", "APPLE_API_ISSUER_ID"]
    missing = [name for name in required if not os.environ.get(name)]
    if missing:
        raise SystemExit("缺少 GitHub Actions Secrets：" + ", ".join(missing))
    if not os.environ["MACOS_SIGNING_IDENTITY"].startswith("Developer ID Application: "):
        raise SystemExit("MACOS_SIGNING_IDENTITY 必须为 Developer ID Application 证书完整名称")
    # 功能：在 Linux 元数据任务提前检查配置，缺凭据不消耗 Mac 构建时间；时间：2026-11-13；作者：lq。
    if options.action == "check":
        print("签名及公证 Secrets 配置项齐全，实际有效性将在 Mac 验证")
        return
    directory.mkdir(mode=0o700)
    certificate = directory / "certificate.p12"
    api_key = directory / "AuthKey.p8"
    certificate.write_bytes(base64.b64decode(os.environ["MACOS_CERTIFICATE_P12_BASE64"], validate=True))
    api_key.write_text(os.environ["APPLE_API_KEY_P8"])
    certificate.chmod(0o600)
    api_key.chmod(0o600)
    previous = shlex.split(command(["security", "list-keychains", "-d", "user"]))
    search_list.write_text(json.dumps(previous))
    password = secrets.token_urlsafe(32)
    command(["security", "create-keychain", "-p", password, str(keychain)])
    command(["security", "set-keychain-settings", "-lut", "21600", str(keychain)])
    command(["security", "unlock-keychain", "-p", password, str(keychain)])
    command(["security", "import", str(certificate), "-k", str(keychain), "-P",
             os.environ["MACOS_CERTIFICATE_PASSWORD"], "-T", "/usr/bin/codesign", "-T", "/usr/bin/security"])
    command(["security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:",
             "-s", "-k", password, str(keychain)])
    command(["security", "list-keychains", "-d", "user", "-s", str(keychain), *previous])
    identities = command(["security", "find-identity", "-v", "-p", "codesigning", str(keychain)])
    if f'"{os.environ["MACOS_SIGNING_IDENTITY"]}"' not in identities:
        raise SystemExit("导入的证书不是指定的有效签名身份，检查是否包含私钥或证书已过期")
    command(["xcrun", "notarytool", "store-credentials", "qds6-notary", "--keychain", str(keychain),
             "--key", str(api_key), "--key-id", os.environ["APPLE_API_KEY_ID"],
             "--issuer", os.environ["APPLE_API_ISSUER_ID"]])
    certificate.unlink()
    api_key.unlink()
    print("临时钥匙串及 Apple 公证凭据已就绪")


if __name__ == "__main__":
    run()
