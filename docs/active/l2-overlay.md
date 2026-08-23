# L2 命令生态:overlay 管线

> 状态:设计中(2026-08-22)。定案背景见 state.md「L2 定案修正」。

## 1. 为什么 apt 直通死了(实拍证伪)

Termux 官方仓库的 deb 把 `/data/data/com.termux` **三处焊死**:

1. data.tar 里的文件路径:`./data/data/com.termux/files/usr/...`(dpkg
   以 instdir=/ 解包 → 敲别家院门 → Permission denied,2026-08-22 实拍);
2. maintainer 脚本(postinst 等):shebang 与内部路径全是 com.termux;
3. 编译期 `--prefix`:二进制运行时找的默认配置路径(部分可忍,见 §5)。

`apt update` 能成(只拉清单不碰文件),`apt install` 必死。结论:**na 的
包安装必须走「重打包 overlay」,apt 只配当依赖解析器用。**

## 2. 架构三段

```
[构建侧:手机真 Termux]          [交接点]              [运行时:na 终端]
scripts/build-overlay.sh  →  共享存储目录(§3)  →  kfm-pkg install <名>
apt 解依赖+下载 deb                              (shell 脚本,$PREFIX/bin)
→ 剥前缀/改脚本/收链接
→ 打 na-overlay-<名>.tar.gz
```

- **构建侧**:`scripts/build-overlay.sh <名> <包...>`,借真 Termux 的
  apt 干活(空 status + 空 cache 双闸骗出完整依赖闭包的下载地址——
  只空 status 会被本机 apt 缓存吞掉主包,2026-08-22 实拍),curl 拉回
  deb),逐个解包重打。产物 = overlay 包,扔交接点。
- **运行时**:`kfm-pkg` 纯 shell 脚本(curl/unzip 都不需要——tar 就够),
  APK assets 自带,每次启动铺进 `$PREFIX/bin/kfm-pkg`(覆盖式,版本随
  APK 自然滚动)。install = 解包到 staging → 铺进 $PREFIX → 建符号链接 →
  跑改写后的 postinst → 登记 `$PREFIX/var/kfm-pkg/installed`。
- **不加新基础设施**:无新端口、无 nginx 改动、无服务器端代码。

## 3. 交接点(2026-08-22 实拍修正)

~~首选共享存储目录~~:**证伪**——na 读 `/storage/emulated/0/工作台`
EACCES(该机 scoped storage 不吃 targetSdk 28 的 legacy 牌;且
Termux 也写不进 na 的 Android/data,共享存储双向都不通)。

定案:**手机本机回环 HTTP**。Termux 侧 `scripts/serve-overlays.sh`
(python3 http.server,只绑 127.0.0.1:8027,根目录 ~/w/kfm-na-overlays/),
na 侧 kfm-pkg 用 bootstrap 自带的 curl 拉包。零新基础设施、不出手机、
不对网卡开口。KFM_OVERLAY_URL 可换地址;本地文件路径留作逃生门。

备选(回环也不通时):kfmv4 8021 加静态挂载——动 kfmv4 仓,最后才走。

## 4. overlay 包格式(tar.gz)

```
payload/…            # usr 相对路径的纯净文件树(bin/ssh 等)
SYMLINKS.txt         # 沿用 bootstrap 格式:target←linkpath(U+2190)
maint/<pkg>/{preinst,postinst,prerm,postrm}   # 路径已改写(§5)
MANIFEST             # name=<名> / packages=<闭包列表> / built=<时间戳>
```

剥前缀规则:`data.tar.*` 解出后,`./data/data/com.termux/files/usr/` 下
的一切上移为 payload 根;symlink 不落地,记进 SYMLINKS.txt 由安装侧建。

## 5. 改写规则(maintainer 脚本)

- `s|/data/data/com.termux/files/usr|$PREFIX|g`
- `s|/data/data/com.termux/files/home|$HOME|g`
- shebang 同上(第一列规则已覆盖)

编译期焊死的运行时路径(§1.3)**不改二进制**,按包记 shim:
- openssh 客户端:系统级 ssh_config 缺失非致命,`~/.ssh` 照常用 → 无需 shim;
- 其他包遇到运行时找 com.termux 路径的,优先用环境变量 shim
  (如 `VIMRUNTIME`),在本文 §6 逐包登记。

## 6. 已装包 shim 登记表

- **openssh(2026-08-23 实拍闭环)**:
  - `known_hosts` 等 `~` 系路径走 getpwuid,焊死 com.termux 家目录
    (EACCES 警告,不挡路)→ shim:`$HOME/.ssh/config` 显式给
    `UserKnownHostsFile $HOME/.ssh/known_hosts`,alias
    `ssh='ssh -F $HOME/.ssh/config'` 写入 `$HOME/.bashrc`。
  - **境外链路掐 DSCP 标记包**(实拍:裸 TCP 通、KEX 后 userauth 段
    abort;`IPQoS=none` 即通)→ shim:同 config 里 `IPQoS none`。
  - 私钥 = 复用 Termux `moliy_key`(用户拍板),经 8027 传递后落
    `$PREFIX/etc/ssh/id_ed25519`(**私有区,不进共享存储 HOME**),
    传递副本用后已删。

## 6.5 运行时安装的铁律(2026-08-22 实拍)

**禁止原地截断覆写 $PREFIX 里的活体文件**。运行中的 shell 映射着
`$PREFIX/lib` 的 .so,`cp -a` 原地覆写会抽空活体进程的内存映射
(装完 base 会话 exit -1 实录)。kfm-pkg 逐文件 `.kfm-new` + `mv`
原子替换:新文件新 inode,老 inode 陪老进程寿终,新进程自然用新文件。

## 7. 考题与判卷

- **host 考题**(`scripts/test-overlay.sh`,挂 chain):fixture 假 deb
  (含焊死路径文件 + symlink + postinst)走管线核心 → 断言前缀剥净、
  脚本改写、SYMLINKS.txt 正确。
- **实拍判卷**:na 终端 `kfm-pkg install openssh` 成 → `ssh -V` 出版本 →
  `ssh` 真连服务器通;`git --version` 同理。
