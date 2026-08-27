#!/bin/bash
# na-case.sh — 开案脚手架(结晶条款配套,2026-08-27,调试闸门.md §十一)
#
#   bash scripts/na-case.sh BAR-040 "一句话现象"
#
# 满足逃逸条款(看不见现场/一修不好/复现不了)的 bug 开案用。
# 一条命令干四件事:
#   ① na-autopsy.sh 收现场(案卷附件,手机不可达不致命);
#   ② bugs.md 案卷区末尾开六栏案卷骨架;
#   ③ scripts/cases/BAR-xxx-repro.sh 复现脚本模板;
#   ④ 打印下一步清单。
# 结案硬条件:六栏填满 + 至少一件永久资产(新观测点/回归考题/可复用脚本)。
set -euo pipefail

if [ $# -lt 2 ] || [[ ! "$1" =~ ^BAR-[0-9]+$ ]]; then
    echo "用法: bash scripts/na-case.sh BAR-040 \"一句话现象\"" >&2
    exit 1
fi
BAR="$1"; DESC="$2"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CASES="$ROOT/scripts/cases"
mkdir -p "$CASES"

echo "== ① 收现场(手机不可达不致命) =="
bash "$ROOT/scripts/na-autopsy.sh" "$BAR" || echo "⚠️  收尸失败(手机不在线?),案卷照常开" >&2

echo "== ② bugs.md 案卷区开骨架 =="
cat >> "$ROOT/docs/ledger/bugs.md" <<EOF

### $BAR 案卷:$DESC

- **现象**:$DESC(开案 $(date +%Y-%m-%d);现场 autopsy/ 内 "$BAR" 目录)
- **复现注入序列**:scripts/cases/$BAR-repro.sh(手势类注明"用户手测")
- **盲区**:(当时为什么看不见——对照调试闸门.md §十一观测矩阵哪格空)
- **长出的新观测点**:(计数器/trace 段/通道/脚本;没有就写"考题代替")
- **考题**:(tests/ 路径 + 名;纯平台胶水写 tests:na 理由)
- **判卷法**:(同一把尺复验的具体命令与通过标准)
EOF

echo "== ③ 复现脚本模板 =="
cat > "$CASES/$BAR-repro.sh" <<'EOF'
#!/bin/bash
# 复现脚本模板(na-case.sh 生成)——把用户描述翻成注入/观测序列。
# 判卷法总纲:修复后用本脚本原样再打一遍,同一把尺复验。
# 契约(逃逸条款机械面):本脚本退出码 = 判卷结果(0=愈/复现未命中,
# 非零=未愈/复现命中)。结案时案卷「判卷法」栏必须引用本脚本;
# C 档感官判卷例外,但案卷须写明实拍步骤清单。
set -euo pipefail
cd "$(dirname "$0")/../.."

# 例:注入命令 → 等执行 → 读屏断言
# bash scripts/na-type.sh 'ls\r'
# sleep 1
# bash scripts/na-text.sh | grep -q '预期输出' || { echo "❌ 复现未命中"; exit 1; }

# 例:拍图亲验
# bash scripts/na-shot.sh   # 落 /tmp/na-shot.png

echo "TODO: 把复现序列填进来"
exit 1  # 模板态恒未愈,防空脚本误判结案
EOF
chmod +x "$CASES/$BAR-repro.sh"

echo "== ④ 下一步清单 =="
cat <<EOF
✅ $BAR 开案完成:
   - 案卷骨架:docs/ledger/bugs.md 案卷区末尾
   - 复现模板:scripts/cases/$BAR-repro.sh
   - 现场附件:autopsy/ 内最新 "$BAR" 目录(若收尸成功)
下一步:排障手册(docs/active/排障手册.md)速查表选尺 → 填复现脚本
→ 复现命中 → 写失败考题 → 修复 → 同尺复验 → 六栏填满结案。
EOF
