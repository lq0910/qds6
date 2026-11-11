#!/usr/bin/env python3
"""功能：挂载 DMG、复制安装并在隔离数据目录启动真实 Mac 应用；时间：2026-11-11；作者：lq。"""
import argparse
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def command(arguments):
    """功能：按参数数组调用系统工具，原样保留失败结果；时间：2026-11-11；作者：lq。"""
    return subprocess.check_output(arguments, cwd=ROOT, stderr=subprocess.STDOUT).decode()


def run():
    """功能：仅验证当前机器复制安装和启动，明确区分干净机器验收；时间：2026-11-11；作者：lq。"""
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    output = args.directory.resolve()
    work = output / "installation-check"
    work.mkdir()
    mount = work / "mounted"
    mount.mkdir()
    helper = work / "verify-window"
    print(command(["xcrun", "swiftc", "-module-cache-path", str(work / "swift-cache"), str(ROOT / "scripts/verify-window.swift"), "-o", str(helper)]))
    dmg = next(output.glob("*.dmg"))
    report = {"scope": "current Mac; fresh copied application and isolated data, not a clean machine", "dmg": dmg.name}
    report["mount"] = command(["hdiutil", "attach", "-readonly", "-nobrowse", "-mountpoint", str(mount), str(dmg)])
    try:
        installed = work / "QDS6.app"
        print(command(["ditto", str(mount / "QDS6.app"), str(installed)]))
        report["signature"] = command(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(installed)])
    finally:
        report["unmount"] = command(["hdiutil", "detach", str(mount)])
    environment = dict(os.environ)
    for key in list(environment):
        if key.startswith("DYLD_"):
            del environment[key]
    environment.update(PATH="/usr/bin:/bin:/usr/sbin:/sbin", QDS6_DATA_DIR=str(work / "data"))
    with (work / "desktop.log").open("w") as log:
        start = time.monotonic()
        process = subprocess.Popen([str(installed / "Contents/MacOS/qds6")], cwd=work, env=environment, stdout=log, stderr=subprocess.STDOUT)
        try:
            while True:
                if process.poll() is not None:
                    raise RuntimeError(f"App exited early: {process.returncode}")
                windows = json.loads(command([str(helper), str(process.pid)]))
                ordinary = [window for window in windows if window["layer"] == 0]
                if ordinary:
                    break
                if time.monotonic() - start > 20:
                    raise RuntimeError("Native window not visible within 20 seconds")
                time.sleep(.1)
            report["process_to_visible_window_ms"] = round((time.monotonic() - start) * 1000)
            report["windows"] = ordinary
            # 功能：只截取自建 QDS6 窗口，系统拒绝屏幕权限时记录失败，不修改系统设置；时间：2026-11-11；作者：lq。
            capture = subprocess.run(["/usr/sbin/screencapture", "-x", "-l", str(ordinary[0]["id"]), str(work / "desktop-window.png")], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            report["screenshot"] = {"exit_code": capture.returncode, "diagnostic": capture.stdout}
            time.sleep(2)
            report["idle_process_sample"] = command(["ps", "-p", str(process.pid), "-o", "pid=,rss=,%cpu=,etime="]).strip()
            database = work / "data/connections.sqlite3"
            with sqlite3.connect(database) as connection:
                report["fresh_profile_count"] = connection.execute("SELECT count(*) FROM profiles").fetchone()[0]
            assert report["fresh_profile_count"] == 0
            assert process.poll() is None
        finally:
            process.terminate()
            process.wait(timeout=10)
    report["bundle_bytes"] = sum(p.stat().st_size for p in installed.rglob("*") if p.is_file())
    report["archives_bytes"] = {p.name: p.stat().st_size for p in output.iterdir() if p.suffix in (".zip", ".dmg")}
    (output / "installation-verification.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    shutil.rmtree(work / "swift-cache", ignore_errors=True)
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    run()
