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
# 别名传播（MAIN0192 楼719／843：同一 Arc 换个名字就漏，例如
#   let t = term_arc.clone();  /  let th = &term_arc;
# 口径限制：不做作用域精确（全局近似），命中仍需人复核。
ALIAS = re.compile(r"^\s*let\s+(?:mut\s+)?(?P<new>\w+)\s*(?::[^=]+)?=\s*&?(?P<old>\w+)(?:\.clone\(\))?\s*;")


def canon(name: str, aliases: dict) -> str:
    seen = set()
    while name in aliases and name not in seen:
        seen.add(name)
        name = aliases[name]
    return name


def scan_file(path: Path):
    lines = path.read_text(errors="ignore").split("\n")
    depth = 0
    aliases = {}  # new -> old（规范名解析）
    guards = []  # (depth, recv, name, lineno)
    hits = []
    for i, line in enumerate(lines, start=1):
        if line.strip().startswith("//"):
            depth += line.count("{") - line.count("}")
            continue
        am = ALIAS.match(line)
        if am and am.group("new") != am.group("old"):
            aliases[am.group("new")] = am.group("old")
        m = GUARD.match(line)
        if m:
            recv, name = m.group("recv"), m.group("name")
            recv = canon(recv, aliases)
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
                recv = canon(t.group("recv"), aliases)
                for gd, grecv, gname, gl in guards:
                    if gd <= depth and grecv == recv:
                        hits.append((path, gl, gname, grecv, i))
        # **逐字符处理花括号**（楼720／832 的教训：整行净差会把闭包的 `}) {` 抵消掉，
        # 导致闭包体内的 guard 被误判为「仍存活」而报假阳性）。逐字符时闭包 `}` 一到，
        # 深度下降，闭包内建立的 guard 当场出域。
        for ch in line:
            if ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth < 0:
                    depth = 0
                guards = [g for g in guards if g[0] <= depth]
    return hits


REQUIRED_FIELDS = ("reason=", "evidence=", "reviewer=", "expires=")


def load_allow():
    """豁免表：模式 + 四字段（reason／evidence／reviewer／expires），缺一即红。

    楼719／720：绑**模式**不绑行号（行号会漂，漂了静默失配）；模式必须命中至少
    一处，否则红；到期未复核也红。返回 ([(pattern, fields)], errors)。
    """
    out, errors = [], []
    if not ALLOW.exists():
        return out, errors
    for raw in ALLOW.read_text().splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        pat, _, meta = line.partition("#")
        pat = pat.strip()
        meta = meta.strip()
        missing = [f for f in REQUIRED_FIELDS if f not in meta]
        if not pat or missing:
            errors.append(f"豁免条目不合规（缺 {'/'.join(missing) or '模式'}）：{line}")
            continue
        fields = {}
        for part in meta.split("|"):
            k, _, v = part.partition("=")
            fields[k.strip()] = v.strip()
        out.append((pat, fields))
    return out, errors


def main() -> int:
    import sys as _sys
    global SRC
    if "--src" in _sys.argv:
        SRC = Path(_sys.argv[_sys.argv.index("--src") + 1])
    all_hits = []
    for p in sorted(SRC.rglob("*.rs")):
        all_hits.extend(scan_file(p))
    allow, allow_errors = load_allow()
    if allow_errors:
        for e in allow_errors:
            print(f"[check-mutex-reentry] ❌ {e}")
        return 1
    import datetime
    import re as _re
    today = datetime.date.today().isoformat()
    unused, expired = [], []
    kept = []
    for hit in all_hits:
        p, gl, gname, grecv, ln = hit
        try:
            rel = p.relative_to(ROOT)
        except ValueError:
            rel = p
        key = f"{rel}:{gl}->{ln}"
        matched = None
        for pat, fields in allow:
            if _re.search(pat, key):
                matched = (pat, fields)
                break
        if matched is None:
            kept.append(hit)
            continue
        if matched[1].get("expires", "") < today:
            expired.append((key, matched[1].get("reviewer", "?"), matched[1].get("expires", "?")))
    for pat, fields in allow:
        def _key(h):
            try:
                r = h[0].relative_to(ROOT)
            except ValueError:
                r = h[0]
            return f"{r}:{h[1]}->{h[4]}"

        if not any(_re.search(pat, _key(h)) for h in all_hits):
            unused.append((pat, fields.get("expires", "?")))
    all_hits = kept
    for p, gl, gname, grecv, ln in all_hits:
        try:
            rel = p.relative_to(ROOT)
        except ValueError:
            rel = p
        print(f"[check-mutex-reentry] ❌ 命中 {rel}:{gl} guard {gname}={grecv}.lock() … → {rel}:{ln} 又取 {grecv}.lock()")
    bad = bool(all_hits)
    for key, reviewer, exp in expired:
        print(f"[check-mutex-reentry] ❌ 豁免已到期未复核：{key}（reviewer={reviewer}，expires={exp}）")
        bad = True
    for pat, exp in unused:
        print(f"[check-mutex-reentry] ❌ 豁免失配（模式没命中任何一处，病灶可能已走）：{pat} expires={exp}")
        bad = True
    if bad:
        return 1
    if allow:
        print(f"[check-mutex-reentry] ℹ️ 豁免 {len(allow)} 条（计数单列，增长需 reviewer 背书）")
    if not all_hits and not expired and not unused:
        print("[check-mutex-reentry] OK — 口径内零命中（直接取锁／同接收者／guard 存活期）")
        return 0
    print(f"[check-mutex-reentry] ❌ 命中 {len(all_hits)} 处（同一接收者的 guard 存活期内再取锁）：")
    for p, gl, gname, grecv, ln in all_hits:
        rel = p.relative_to(ROOT)
        print(f"  {rel}:{gl} guard {gname}={grecv}.lock() … → {rel}:{ln} 又取 {grecv}.lock()")
    return 1


if __name__ == "__main__":
    sys.exit(main())
