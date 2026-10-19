#!/bin/bash
# test-mutex-reentry.sh — 同 Arc 重入检查器的别名传播与负对照（MAIN0192 楼719／843）
#
# 两具夹具：
#   ① 别名重入：let alias = term_arc.clone() 后 alias.lock() 持锁，再 term_arc.lock()
#      ⇒ 必须命中（同一对象不同名，靠别名传播抓到）
#   ② 顺序取锁：前后两次 lock 之间显式 drop ⇒ 必须不命中（负对照）
set -u
CHK="$(cd "$(dirname "$0")" && pwd)/check/check-mutex-reentry.py"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
fails=0

mk() { mkdir -p "$1"; cat >"$1/a.rs"; }

mk "$tmp/pos/src" <<'RS'
fn f(term_arc: std::sync::Arc<std::sync::Mutex<u8>>) {
    let alias = term_arc.clone();
    let g = alias.lock().unwrap();
    {
        let h = term_arc.lock().unwrap();
    }
}
RS
mk "$tmp/neg/src" <<'RS'
fn f(term_arc: std::sync::Arc<std::sync::Mutex<u8>>) {
    let g = term_arc.lock().unwrap();
    drop(g);
    let h = term_arc.lock().unwrap();
}
RS

chk() { # chk <期望 rc> <说明> <src 目录>
  local want=$1 desc=$2 dir=$3
  python3 "$CHK" --src "$dir" >"$tmp/out" 2>&1
  local got=$?
  if [ "$got" -ne "$want" ]; then
    echo "❌ $desc：期望 rc=$want，实得 rc=$got"; sed 's/^/    /' "$tmp/out"; fails=$((fails+1))
  else
    echo "✓ $desc"
  fi
}

chk 1 "别名重入要命中（不同名同一对象）" "$tmp/pos/src"
chk 0 "显式 drop 后取锁不命中（负对照）" "$tmp/neg/src"

if [ "$fails" -ne 0 ]; then echo "[test-mutex-reentry] $fails 挂"; exit 1; fi
echo "[test-mutex-reentry] 2 过 / 0 红"
