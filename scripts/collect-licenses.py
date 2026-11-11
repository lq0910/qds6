#!/usr/bin/env python3
"""功能：保留精确依赖的原始许可和来源，生成 Mac 包 NOTICE；时间：2026-11-11；作者：lq。"""
import argparse
import concurrent.futures
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
NAMES = re.compile(r"^(licen[cs]e|copying|notice|copyright)([-._].*)?$", re.I)


def run():
    """功能：仅普通/构建依赖闭包；缺失原文只从上游精确提交下载，不执行来源；时间：2026-11-11；作者：lq。"""
    parser = argparse.ArgumentParser()
    parser.add_argument("--fetch-missing", action="store_true")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", "aarch64-apple-darwin"], cwd=ROOT))
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    seen = set()

    def visit(identity):
        if identity in seen:
            return
        seen.add(identity)
        for dependency in nodes[identity]["deps"]:
            if any(k["kind"] in (None, "build") for k in dependency["dep_kinds"]):
                visit(dependency["pkg"])

    visit(next(p["id"] for p in packages.values() if p["name"] == "qds6-app"))
    destination = ROOT / "output/third-party"
    destination.mkdir(parents=True, exist_ok=True)
    upstream_cache = ROOT / "output/license-upstream"
    upstream_cache.mkdir(parents=True, exist_ok=True)

    def collect(identity):
        package = packages[identity]
        if package["source"] is None:
            return None
        source = Path(package["manifest_path"]).parent
        label = f'{package["name"]}-{package["version"]}'
        target = destination / label
        target.mkdir(exist_ok=True)
        paths = [p for p in source.rglob("*") if p.is_file() and NAMES.match(p.name)]
        # 功能：Git workspace 的继承许可沿真实父目录查找；时间：2026-11-11；作者：lq。
        if package["source"].startswith("git+"):
            parent = source.parent
            for _ in range(3):
                paths.extend(p for p in parent.iterdir() if p.is_file() and NAMES.match(p.name))
                parent = parent.parent
        texts = []
        for path in sorted(set(paths)):
            data = path.read_bytes()
            if len(data) > 2 * 1024 * 1024:
                raise RuntimeError(f"Unusually large license: {path}")
            relative = str(path.relative_to(source)) if path.is_relative_to(source) else path.name
            texts.append(f"\n===== {relative} =====\n" + data.decode("utf-8", "replace"))
        origins = []
        if not texts:
            vcs_file = source / ".cargo_vcs_info.json"
            vcs = json.loads(vcs_file.read_text()) if vcs_file.exists() else {}
            commit = vcs.get("git", {}).get("sha1")
            repository = package.get("repository") or (package.get("homepage") or "").split("/tree/")[0]
            if repository and commit:
                repository = repository.removesuffix(".git").rstrip("/")
                if repository.startswith("https://github.com/"):
                    base = repository.replace("https://github.com/", "https://raw.githubusercontent.com/") + f"/{commit}/"
                else:
                    base = repository + f"/-/raw/{commit}/"
                prefix = vcs.get("path_in_vcs", "")
                prefixes = [prefix + "/", ""] if prefix else [""]
                candidates = ["LICENSE", "LICENSE.md", "LICENSE-MIT", "LICENSE-MIT.txt", "LICENSE-APACHE", "LICENSE.txt", "COPYING", "LICENSES/MIT.txt", "LICENSES/Apache-2.0.txt", "LICENSE-MIT.md", "LICENSE-APACHE.md"]
                for prefix in prefixes:
                    for candidate in candidates:
                        url = base + prefix + candidate
                        cache = upstream_cache / (hashlib.sha256(url.encode()).hexdigest() + ".txt")
                        if cache.exists():
                            data = cache.read_bytes()
                        elif args.fetch_missing:
                            try:
                                with urllib.request.urlopen(url, timeout=12) as response:
                                    data = response.read(2 * 1024 * 1024 + 1)
                            except (urllib.error.URLError, TimeoutError):
                                continue
                            if len(data) > 2 * 1024 * 1024 or data.lstrip().startswith(b"<!DOCTYPE"):
                                continue
                            cache.write_bytes(data)
                        else:
                            continue
                        texts.append(f"\n===== {url} =====\n" + data.decode("utf-8", "replace"))
                        origins.append(url)
                        break
                    if texts:
                        break
        # 功能：四个旧 crate 未附许可文件/原提交不可读，明确标注规范文本补充并附未修改源码，不冒称上游原文；时间：2026-11-11；作者：lq。
        supplemental = {"mac-0.1.1": "Apache-2.0", "pathfinder_geometry-0.5.1": "Apache-2.0", "seahash-4.1.0": "MIT", "simd_helpers-0.1.0": "MIT"}
        needs_source = label in supplemental
        if not texts and needs_source:
            chosen = supplemental[label]
            attribution = ", ".join(package.get("authors", []))
            introduction = f"Supplemental standard {chosen} text; upstream crate declares {package['license']}. Packaged source omits license file or original commit is unavailable. Author attribution from unchanged Cargo manifest: {attribution}. Full original crate source included. This is not represented as a recovered upstream license file.\n\n"
            if chosen == "Apache-2.0":
                body = (ROOT / "LICENSE").read_text()
            else:
                body = "Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the \"Software\"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:\n\nThe above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.\n"
            texts.append(introduction + body)
        if not texts:
            raise RuntimeError(f"Missing upstream license text: {label}")
        (target / "LICENSES.txt").write_text("\n".join(texts))
        for name in ("Cargo.toml.orig", "README.md"):
            if (source / name).exists():
                shutil.copyfile(source / name, target / name)
        # 功能：随包提供 MPL 依赖未经修改的源码，保留原版权与许可；时间：2026-11-11；作者：lq。
        if "MPL" in (package.get("license") or "") or needs_source:
            with tarfile.open(target / "source.tar.gz", "w:gz") as archive:
                archive.add(source, arcname=label)
        return {"name": package["name"], "version": package["version"], "license": package["license"], "repository": package.get("repository"), "source": package["source"], "upstream_license_urls": origins, "supplemental_standard_license": supplemental.get(label)}

    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        entries, errors = [], []
        for future in concurrent.futures.as_completed([pool.submit(collect, i) for i in sorted(seen)]):
            try:
                entry = future.result()
                if entry: entries.append(entry)
            except Exception as error:
                errors.append(str(error))
        if errors: raise RuntimeError("\n".join(sorted(errors)))
        entries.sort(key=lambda e: (e["name"], e["version"]))
    (destination / "dependencies.json").write_text(json.dumps(entries, ensure_ascii=False, indent=2) + "\n")
    header = "# QDS6 third-party notices\n\nGenerated 2026-11-11 by lq. Exact macOS normal/build dependency closure; build tools may not be linked. Original available notices are retained in each directory. SPDX OR alternatives retain the upstream choice; this inventory does not relicense dependencies. MPL source archives are included unmodified. Four old crates with missing license files have clearly labeled supplemental standard texts and unmodified source archives: mac 0.1.1, pathfinder_geometry 0.5.1, seahash 4.1.0, simd_helpers 0.1.0. This is an internal build inventory; public distribution still needs final license review.\n\n"
    rows = "| Package | Version | Declared license |\n| --- | --- | --- |\n"
    rows += "\n".join(f'| {e["name"]} | {e["version"]} | {e["license"]} |' for e in entries)
    (destination / "NOTICE.md").write_text(header + rows + "\n")
    print(f"Collected original licenses for {len(entries)} dependencies: {destination}")


if __name__ == "__main__":
    run()
