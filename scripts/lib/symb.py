#!/usr/bin/env python3
# 在手机侧就地符号化 crash-stack.bin 里的 libkfm_na 帧
# 用法: python3 symb.py <stack.bin> <crash-maps> <libkfm_na.so> [dump偏移]
import re, struct, subprocess, bisect, sys

stackf, mapsf, so = sys.argv[1], sys.argv[2], sys.argv[3]
only_off = int(sys.argv[4]) if len(sys.argv) > 4 else None

data = open(stackf, 'rb').read()
maps = open(mapsf).read().splitlines()
execs = []
for l in maps:
    mm = re.match(r'([0-9a-f]+)-([0-9a-f]+) (\S+) (\S+).*\s(\S*)$', l)
    if mm and 'x' in mm.group(3):
        execs.append((int(mm.group(1), 16), int(mm.group(2), 16),
                      int(mm.group(4), 16), (mm.group(5) or '[anon]').split('/')[-1]))
ours = [e for e in execs if 'libkfm_na' in e[3]]

out = subprocess.run(["nm", "--defined-only", so], capture_output=True, text=True).stdout
syms = []
for l in out.splitlines():
    p = l.split(None, 2)
    if len(p) >= 3:
        try: syms.append((int(p[0], 16), p[2]))
        except ValueError: pass
syms.sort()
vals = [s[0] for s in syms]
def sym(v):
    i = bisect.bisect_right(vals, v) - 1
    return f"{syms[i][1]}+{hex(v - syms[i][0])}" if i >= 0 else '?'

hdrs = list(re.finditer(rb'DUMP tid=\d+ sp=0x[0-9a-f]+ len=\d+\n', data))
for m in hdrs:
    if only_off is not None and m.start() != only_off:
        continue
    h = m.group(0).decode().strip()
    stk = data[m.end():m.end() + 16384]
    print(f"==== @{m.start()} {h} ====")
    for i in range(0, len(stk) - 8, 8):
        v = struct.unpack('<Q', stk[i:i + 8])[0]
        for lo, hi, fo, name in execs:
            if lo <= v < hi:
                if 'libkfm_na' in name:
                    # 我们库 r-xp 段文件偏移 0: vaddr = v - lo
                    print(f"+0x{i:04x} {hex(v)} OUR {sym(v - lo)}")
                else:
                    print(f"+0x{i:04x} {hex(v)} {name} +{hex(v - lo + fo)}")
                break
