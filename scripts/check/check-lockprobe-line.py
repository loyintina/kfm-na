#!/usr/bin/env python3
"""check-lockprobe-line.py — 探针报行的**文本层**校验器（MAIN0192 楼739）

为什么在文本层：缺字段的用例在 pub 签名上不可表达（tok／begin_ms 由内部生成），
但契约条款必须被验。白露楼739 定的位置就是这里——对表脚本吃**手工造的缺字段行**，
不需要 Rust 入口，也不逼实现放宽签名。

用法：
  python3 scripts/check/check-lockprobe-line.py <行文件>      # 每行一条探针报行
  python3 scripts/check/check-lockprobe-line.py --self-test   # 三条夹具行自测

判据（契约 v4）：
  probe=lock 行必须含 12 个字段且**顺序**一致 → 否则 invalid(reason=缺的第一个字段)
  probe=invalid 行必须含 schema= 与 reason= → 这是合法行，不算无效
  probe=lock mode=init 行必须含 schema="v4"
退出码：0 全合规；1 有不合规行
"""
import sys
from pathlib import Path

FIELDS = [
    "probe=lock", "mode=", "mono_ms=", "begin_ms=", "frame=",
    "site=", "arc=", "tok=", "holder=", "verdict=", "web_on_top=", "ai_snap=",
]
SCHEMA = "v4"

# **覆盖面做成必填字段 dims**（楼759：自由文本后缀照样靠人记得写；dims 是机器可核的
# 必填项，缺失即落 invalid reason=missing_field field=dims，复用 v4 的缺字段机制）。
# 语义有效性不归本校验器（归契约定）。
# **词表归契约**（楼764／766）：工具**不内置**任何 dims 值，只从契约文件读；
# 文件缺失或格式坏 ⇒ 拒跑（rc=2，第三态），不许回退到内置词表。
VOCAB = Path(__file__).resolve().parent / "lockprobe-dims-vocab.txt"


def load_vocab():
    """返回 (vocab_str, None) 或 (None, 拒跑原因)"""
    if not VOCAB.exists():
        return None, "vocab_missing"
    vals = []
    for raw in VOCAB.read_text().splitlines():
        t = raw.split("#")[0].strip()
        if not t:
            continue
        if not all(c.isalnum() or c in "_-" for c in t):
            return None, "vocab_malformed"
        vals.append(t)
    if not vals:
        return None, "vocab_empty"
    return ",".join(vals), None


# **两套闭集，各自机械强制**（楼779／781／782）：
# 行级 = pass / invalid / init / instrument_error —— 「行坏」与「判不了」必须分开，
#        否则仪器故障会被记成行有问题；
# 列级 = judged / UNCHECKED —— 只许出现在列级统计，**行输出里出现即格式错**。
ROW_STATES = ("pass", "invalid", "init", "instrument_error")
COL_STATES = ("judged", "UNCHECKED")


def assert_row_state(st: str) -> None:
    if st not in ROW_STATES:
        raise SystemExit(
            f"INVALID reason=bad_value field=row_state expect={'|'.join(ROW_STATES)} got={st}"
        )
    if st in COL_STATES:
        raise SystemExit(
            f"INVALID reason=bad_value field=row_state got={st}（UNCHECKED 是列级状态，不许进行输出）"
        )


_SEQ = [0]


def _now_stamp() -> str:
    """时刻戳两段：wall（跨机对表，先核偏差）＋进程内单调序号（同秒 tie-break）。
    取最新一律按 (wall, seq) 排，不按文件顺序（楼773／774）。"""
    import datetime
    _SEQ[0] += 1
    t = datetime.datetime.now().astimezone().isoformat(timespec="seconds")
    return f"t={t} seq={_SEQ[0]}"


def vocab_stamp() -> str:
    """仪器戳（楼772）：每条结论带词表可用性与版本，这样「同一行两个结论」是两次
    测量而不是矛盾；转变时不回改旧行，只追加新行。"""
    vocab, why = load_vocab()
    if vocab is None:
        return f"vocab=unavailable({why}) {_now_stamp()}"
    import hashlib

    ver = hashlib.sha256(vocab.encode()).hexdigest()[:8]
    return f"vocab=available@{ver} {_now_stamp()}"


def emit_instrument() -> None:
    """仪器状态**单列一行**（楼769）：行属性只判存在性/形态；值合规与否取决于词表，
    词表缺失就记「未覆盖」，不把 undefined 写进结论态。"""
    vocab, why = load_vocab()
    if vocab is None:
        print(
            f"[check-lockprobe-line] INSTRUMENT state=unavailable reason={why} "
            f"uncovered={"all_value_dims"} schema={SCHEMA}"
        )
    else:
        print(
            f"[check-lockprobe-line] INSTRUMENT state=available dims={vocab} schema={SCHEMA}"
        )


def emit_invalid(reason: str, field: str, detail: str) -> None:
    """所有拒绝都走这一条：机器可读，带 reason／field（楼759：缺 dims 也是缺字段）"""
    print(f"INVALID reason={reason} field={field} {detail}")


def check_line(line: str):
    """返回 None（合规）或 (reason, 说明)"""
    s = line.strip()
    if not s:
        return None
    if "dims=" in s and not s.split("dims=")[1].split()[0].strip():
        return ("missing_field", f"dims 为空: {s}")
    if s.startswith("probe=invalid"):
        for need in ("schema=", "reason="):
            if need not in s:
                return ("invalid_line_malformed", f"invalid 行缺 {need}: {s}")
        return None
    if not s.startswith("probe=lock"):
        return ("unknown_kind", f"不是探针行: {s}")
    if "mode=init" in s:
        if f"schema={SCHEMA}" not in s:
            return ("init_schema", f"init 行 schema 必须是 {SCHEMA}: {s}")
        return None
    cursor = 0
    for f in FIELDS:
        at = s.find(f, cursor)
        if at < 0:
            return ("missing_field", f"缺字段或顺序错 {f}: {s}")
        cursor = at + len(f)
    return None


def main() -> int:
    if "--fixture" in sys.argv:
        # 共用夹具模式（楼744／750／752／754）：**二维期望表**——行类型 × 判据，
        # 一行可同时挂两类判据，所以字段用 | 分隔、各自具名（kind=／expect=／format=）
        from pathlib import Path as _P
        fp = _P(sys.argv[sys.argv.index("--fixture") + 1])
        bad = 0
        n = 0
        for raw in fp.read_text().splitlines():
            raw = raw.strip()
            if not raw or raw.startswith("#"):
                continue
            parts = [p.strip() for p in raw.split("|")]
            line = parts[0]
            meta = {}
            for kv in parts[1:]:
                k, _, v = kv.partition("=")
                meta[k.strip()] = v.strip()
            n += 1
            got = check_line(line)
            got_kind = "ok" if got is None else got[0]
            want = meta.get("expect", "")
            if got_kind != want:
                print(f"❌ 夹具第 {n} 条 判据 expect：期望 {want}，实得 {got_kind} — {line}")
                bad += 1
            # 第二维：行类型（缺 kind 即红——楼754：两类判据必须分行写）
            want_kind = meta.get("kind")
            if not want_kind:
                print(f"❌ 夹具第 {n} 条 缺 kind（二维表必须写明行类型）— {line}")
                bad += 1
                continue
            actual_kind = "init" if "mode=init" in line else (
                "invalid" if line.startswith("probe=invalid") else (
                    "self" if "verdict=self" in line else (
                        "contend" if "verdict=contend" in line else (
                            "end" if "verdict=end" in line else "lock"))))
            if actual_kind != want_kind:
                print(f"❌ 夹具第 {n} 条 判据 kind：期望 {want_kind}，实得 {actual_kind} — {line}")
                bad += 1
        if bad:
            print(f"[check-lockprobe-line] 夹具 {bad} 挂 / {n} 条")
            return 1
        emit_instrument()
        print(f"[check-lockprobe-line] OK fixture={n} schema={SCHEMA} {vocab_stamp()}")
        return 0

    if "--self-test" in sys.argv:
        good = ("probe=lock mode=natural mono_ms=123 begin_ms=123 frame=7 site=s arc=a "
                "tok=1 holder=0 verdict=end web_on_top=1 ai_snap=0")
        miss_frame = good.replace("frame=7 ", "")
        miss_begin = good.replace("begin_ms=123 ", "")
        inv = "probe=invalid schema=v4 reason=frame"
        init = "probe=lock mode=init schema=v4"
        cases = [
            ("正常行", good, None),
            ("缺 frame", miss_frame, "missing_field"),
            ("缺 begin_ms", miss_begin, "missing_field"),
            ("invalid 行合法", inv, None),
            ("init 行合法", init, None),
            ("init 缺 schema", "probe=lock mode=init", "init_schema"),
        ]
        bad = 0
        for name, line, want in cases:
            got = check_line(line)
            got_kind = None if got is None else got[0]
            if got_kind != want:
                print(f"❌ {name}: 期望 {want}，实得 {got_kind}")
                bad += 1
            else:
                print(f"✓ {name}")
        if bad:
            print(f"[check-lockprobe-line] {bad} 挂")
            return 1
        emit_instrument()
        print(f"[check-lockprobe-line] OK tests=6 schema=v4 {vocab_stamp()}")
        return 0

    if len(sys.argv) < 2:
        print("用法: check-lockprobe-line.py <行文件> | --self-test", file=sys.stderr)
        return 1
    path = Path(sys.argv[1])
    bad = 0
    for i, line in enumerate(path.read_text(errors="ignore").splitlines(), start=1):
        r = check_line(line)
        if r is not None:
            print(f"❌ 第 {i} 行 {r[0]}：{r[1]}")
            emit_invalid(r[0], r[0], f"line={i}")
            bad += 1
    if bad:
        print(f"[check-lockprobe-line] {bad} 行不合规（共 {i} 行）")
        return 1
    emit_instrument()
    print(f"[check-lockprobe-line] OK lines={i} schema={SCHEMA} {vocab_stamp()}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
