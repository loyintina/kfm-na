#!/usr/bin/env python3
"""test-relay-timeout.py — BAR-109 回归钉：relay 不得掐静默长连接。

命案（2026-09-19）：redroid-report-relay 的 create_connection(TARGET,
timeout=4) 把 4s 超时留在 socket 上，pump 的 recv 静默 4s 抛 timeout
（OSError 子类）→ 连接被掐。redroid 上 na 的全部 8021 TCP（终端 ws +
exec ws + 报表）都过本接力——attach 静画面 tmux 会话 4.1s 准时「断线」，
na 误判死亡自动重连回默认会话（附着状态与实际错位）。

钉法（行为级，不 grep 源码）：
  ① dummy 上游：收连接 → 静默 8s → 回 'ok'（模拟静画面长会话）
  ② 真起 relay 进程实例（RELAY_* 环境变量指随机高端口，防撞生产实例）
  ③ 客户端经 relay 连上游，静默 6s（越过旧 4s 死亡线）后一问一答
  通过 = 6s 静默后连接活着且字节正确；旧码 = 4.0s 连接被掐（recv EOF）。
"""
import os
import socket
import subprocess
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
RELAY = os.path.join(HERE, "redroid-report-relay.py")


def free_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


def dummy_upstream(port: int, ready: threading.Event):
    """静默 8 秒后回 'ok' 的上游（模拟 attach 后画面静止的 tmux 会话）。
    循环 accept：relay 每接一个客户端就新连一次上游（含监听探测连接）。"""
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", port))
    srv.listen(4)
    ready.set()

    def serve(conn: socket.socket):
        try:
            time.sleep(8)
            conn.sendall(b"ok")
            time.sleep(0.5)
        except OSError:
            pass
        finally:
            conn.close()

    while True:
        try:
            conn, _ = srv.accept()
        except OSError:
            return
        threading.Thread(target=serve, args=(conn,), daemon=True).start()


def main() -> int:
    up_port = free_port()
    relay_port = free_port()
    ready = threading.Event()
    threading.Thread(target=dummy_upstream, args=(up_port, ready), daemon=True).start()
    ready.wait(3)

    env = dict(
        os.environ,
        RELAY_LISTEN_HOST="127.0.0.1",
        RELAY_LISTEN_PORT=str(relay_port),
        RELAY_TARGET_HOST="127.0.0.1",
        RELAY_TARGET_PORT=str(up_port),
    )
    proc = subprocess.Popen(
        [sys.executable, RELAY],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        # 等 relay 起监听（accept 循环就绪）
        deadline = time.time() + 5
        while True:
            try:
                probe = socket.create_connection(("127.0.0.1", relay_port), timeout=0.3)
                probe.close()
                break
            except OSError:
                if time.time() > deadline:
                    print("❌ BAR-109 钉：relay 5s 内没起监听")
                    return 1
                time.sleep(0.1)

        t0 = time.time()
        s = socket.create_connection(("127.0.0.1", relay_port), timeout=3)
        time.sleep(6)  # 越过旧 4s 死亡线
        s.settimeout(4)
        try:
            data = s.recv(16)
        except OSError as e:
            print(f"❌ BAR-109 回潮：6s 静默后连接被掐（+{time.time()-t0:.1f}s, {e}）")
            return 1
        if data != b"ok":
            print(f"❌ BAR-109 钉：6s 静默后收到 {data!r}（EOF=连接已死=回潮）")
            return 1
        print(f"✅ BAR-109 钉绿：6s 静默后连接存活，字节正确（+{time.time()-t0:.1f}s）")
        return 0
    finally:
        proc.terminate()
        proc.wait(3)


if __name__ == "__main__":
    sys.exit(main())
