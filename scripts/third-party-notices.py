#!/usr/bin/env python3
"""Writes THIRD_PARTY_NOTICES.md: every crate linked into the renderer, with its license and license texts.

Build-time-only crates (build scripts and procedural macros) aren't in the executable and aren't listed.

  python3 scripts/third-party-notices.py
"""
import glob
import json
import os
import subprocess

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
meta = json.loads(subprocess.run(
    ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", "x86_64-unknown-linux-musl"],
    cwd=os.path.join(root, "renderer"), check=True, capture_output=True, text=True).stdout)
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
packages = {p["id"]: p for p in meta["packages"]}


def proc_macro(pkg):
    return any("proc-macro" in t["kind"] for t in pkg["targets"])


linked, stack = set(), [meta["resolve"]["root"]]
while stack:
    for dep in nodes[stack.pop()]["deps"]:
        normal = any(k["kind"] is None for k in dep["dep_kinds"])
        if normal and dep["pkg"] not in linked and not proc_macro(packages[dep["pkg"]]):
            linked.add(dep["pkg"])
            stack.append(dep["pkg"])

out = ["# Third-party notices\n\n",
       "`bin/screensafe-x86_64` and `bin/screensafe-aarch64` embed [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) Regular 2.304, "
       "Copyright 2020 The JetBrains Mono Project Authors, under the SIL Open Font License 1.1 "
       "([renderer/fonts/OFL.txt](renderer/fonts/OFL.txt)).\n\n",
       "The Tokyo Drip pack's sign lettering, `packs/tokyo-drip/fonts/sign.otf`, is a renamed subset of "
       "[Noto Sans Mono CJK JP](https://github.com/notofonts/noto-cjk) Bold 2.004, Copyright 2014-2021 Adobe, under the SIL Open "
       "Font License 1.1 ([packs/tokyo-drip/fonts/OFL.txt](packs/tokyo-drip/fonts/OFL.txt)).\n\n",
       "They are built from the same Rust crates on both architectures, each used under the license shown (where a crate offers a choice, "
       "under any one of them). Their license files follow.\n\n",
       "| Crate | Version | License |\n|---|---|---|\n"]
texts = []
for pid in sorted(linked, key=lambda i: (packages[i]["name"], packages[i]["version"])):
    pkg = packages[pid]
    out.append(f"| [{pkg['name']}](https://crates.io/crates/{pkg['name']}) | {pkg['version']} | {pkg['license']} |\n")
    folder = os.path.dirname(pkg["manifest_path"])
    files = sorted(f for f in glob.glob(os.path.join(folder, "*"))
                   if os.path.basename(f).upper().startswith(("LICENSE", "LICENSE", "COPYING", "NOTICE", "UNICODE")) and os.path.isfile(f))
    for f in files:
        texts.append((f"{pkg['name']} {pkg['version']}: {os.path.basename(f)}", open(f, encoding="utf-8", errors="replace").read().strip()))
out.append("\n## License texts\n\nEach text appears once, under every crate and file it came from.\n")
grouped = {}
for title, text in texts:
    grouped.setdefault(text, []).append(title)
for text, titles in grouped.items():
    out.append("\n### " + "; ".join(titles) + f"\n\n```text\n{text}\n```\n")
with open(os.path.join(root, "THIRD_PARTY_NOTICES.md"), "w") as f:
    # Trailing spaces in a license file carry no meaning; dropping them keeps the file clean in diffs.
    f.write("\n".join(line.rstrip() for line in "".join(out).split("\n")))
print(len(linked), "crates,", len(texts), "license files")
