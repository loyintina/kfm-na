#!/usr/bin/env python3
"""check-mutex-reentry.py — 属性型静态检查：同一 Arc 在同一持锁区间内被取第二次

来历（MAIN0192 楼696／697／700）：「三处」原是「函数体内 lock() 调用点计数」，
不是「重入点计数」；行号只作指针，不作判据。判法必须**按属性**：某个 guard
（`let X = <接收者>.lock()...;`）在它存活的作用域内，是否又对**同一接收者**取锁。

口径（写死，免得又被当成全称结论）：
  1. 只看**直接**取锁（`<recv>.lock()` / `<recv>.lock().unwrap()`），不追被调函数内部；
  2. 接收者按**标识符**认（term_arc / th / t 等）；同名不同对象会误报，故报告里
     一律给行号让人复核；
  3. 只判 Rust 源（src/**/*.rs），不判 crates/。

用法：
  python3 scripts/check/check-mutex-reentry.py [--all]      # 默认只报命中
退出码：0 无命中；1 有命中（可作 chain 闸）
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / "src"
ALLOW = ROOT / "scripts" / "check" / "mutex-reentry-allow.txt"

# guard 绑定：let [mut] NAME = [self.]RECV.lock()...;   （行尾分号 = 临时/绑定均可，
# 但只要它绑定成变量，就当成 guard 存活到作用域结束）
# 只有「绑定成变量且语句就此结束」的才是 guard（`let x = a.lock().unwrap().method()`
# 那种是取值，guard 在语句末就放掉了——不是持锁区间，误报就是这么来的）
GUARD = re.compile(
    r"^\s*let\s+(?:mut\s+)?(?P<name>\w+)\s*=\s*(?:self\.)?(?P<recv>\w+)\.lock\(\)(?:\s*\.unwrap\(\))?\s*;\s*$"
)
# 任意直接取锁
TAKE = re.compile(r"(?P<recv>\w+)\.lock\(\)")


def scan_file(path: Path):
    lines = path.read_text(errors="ignore").split("\n")
    depth = 0
    guards = []  # (depth, recv, name, lineno)
    hits = []
    for i, line in enumerate(lines, start=1):
        if line.strip().startswith("//"):
            depth += line.count("{") - line.count("}")
            continue
        m = GUARD.match(line)
        if m:
            recv, name = m.group("recv"), m.group("name")
            # 同一接收者已有存活 guard ⇒ 这就是重入
            for gd, grecv, gname, gl in guards:
                if gd <= depth and grecv == recv:
                    hits.append((path, gl, gname, grecv, i))
            guards.append((depth, recv, name, i))
        # 显式 drop(guard) 也算出作用域（`drop(pg);` 之后对同一接收者取锁是合法的）
        for dm in re.finditer(r"\bdrop\(\s*(\w+)\s*\)", line):
            guards = [g for g in guards if g[2] != dm.group(1)]
        if not m:
            for t in TAKE.finditer(line):
                recv = t.group("recv")
                for gd, grecv, gname, gl in guards:
                    if gd <= depth and grecv == recv:
                        hits.append((path, gl, gname, grecv, i))
        depth += line.count("{") - line.count("}")
        guards = [g for g in guards if g[0] <= depth]
        if depth < 0:
            depth = 0
    return hits


def load_allow():
    """豁免清单：每行 `文件:行号  # 理由`。空豁免不许——每条都要有理由。"""
    out = {}
    if not ALLOW.exists():
        return out
    for raw in ALLOW.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        key, _, reason = line.partition("#")
        key = key.strip()
        reason = reason.strip()
        assert reason, f"豁免必须带理由：{raw}"
        out[key] = reason
    return out


def main() -> int:
    all_hits = []
    for p in sorted(SRC.rglob("*.rs")):
        all_hits.extend(scan_file(p))
    allow = load_allow()
    kept = []
    for hit in all_hits:
        p, gl, gname, grecv, ln = hit
        key = f"{p.relative_to(ROOT)}:{gl}->{ln}"
        if key in allow:
            continue
        kept.append(hit)
    all_hits = kept
    if not all_hits:
        print("[check-mutex-reentry] OK — 口径内零命中（直接取锁／同接收者／guard 存活期）")
        return 0
    print(f"[check-mutex-reentry] ❌ 命中 {len(all_hits)} 处（同一接收者的 guard 存活期内再取锁）：")
    for p, gl, gname, grecv, ln in all_hits:
        rel = p.relative_to(ROOT)
        print(f"  {rel}:{gl} guard {gname}={grecv}.lock() … → {rel}:{ln} 又取 {grecv}.lock()")
    return 1


if __name__ == "__main__":
    sys.exit(main())
