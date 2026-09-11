#!/usr/bin/env python3
"""redroid-report-relay.py — redroid 报表通道兜底接力（2026-09-12）。

病灶：adb 37.0.1 ↔ redroid12 adbd 的 reverse 数据面死了（注册成功、
监听在、连接被 accept，但数据永不转发到 host——四种复位法全试过：
重加 reverse / kill-server / ctl.restart adbd / docker restart，均黑孔）。
NA 报表硬编码 127.0.0.1:8021，容器内必须有本地监听者把流量接出去。

拓扑：
  容器内 127.0.0.1:8021（nc 环接力，见 redroid-up.sh ④.5）
    → 172.18.0.1:8021（本脚本，docker 网桥 host 侧）
    → 127.0.0.1:8021（kfmv4 /kfmv4/api/na-report）

安全面：只听 docker 网桥 IP，公网/局域网不可达（bridge 内部地址）。
无外部依赖，stdlib only。daemon 化由调用方负责（nohup &）。
"""

import socket
import threading

LISTEN_HOST = "172.18.0.1"
LISTEN_PORT = 8021
TARGET = ("127.0.0.1", 8021)
UPSTREAM_TIMEOUT = 4  # 与 NA try_post 的 connect 2s + 读等待节奏对齐


def pump(src: socket.socket, dst: socket.socket):
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            dst.sendall(data)
    except OSError:
        pass
    finally:
        # 静默关闭；只在启动/上游失败时出声（报表通道自身不该吵）
        for s in (src, dst):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            try:
                s.close()
            except OSError:
                pass


def main():
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind((LISTEN_HOST, LISTEN_PORT))
    srv.listen(32)
    print(f"[relay] {LISTEN_HOST}:{LISTEN_PORT} → {TARGET[0]}:{TARGET[1]} 上线", flush=True)
    while True:
        conn, addr = srv.accept()
        try:
            up = socket.create_connection(TARGET, timeout=UPSTREAM_TIMEOUT)
        except OSError as e:
            print(f"[relay] 上游连接失败 {e}", flush=True)
            conn.close()
            continue
        threading.Thread(target=pump, args=(conn, up), daemon=True).start()
        threading.Thread(target=pump, args=(up, conn), daemon=True).start()


if __name__ == "__main__":
    main()
