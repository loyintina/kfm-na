# 内置字体

## 编译期选择机制（BAR-021，2026-08-18）

`build.rs` 编译期二选一，源码树零生成物：

- `local/main.ttf` 存在 → 主字体用它（**本机商业字体，gitignore
  钉死永不进库**；chain.sh 第 1 步防泄漏闸机械执法）
- 主字体占位 = DejaVuSansMono.ttf；CJK/符号 fallback 恒定 =
  FusionPixelMono12-gb2312.ttf（BAR-022：商业美术字体天然缺终端符号，
  fallback 的职责就是补盲文/方块/几何符号——主字体缺的字形由
  prefer_cjk 逐字路由给它）

生产启动**零探测**：不读 /system/fonts，TermView 毫秒级建成
（BAR-020 启动慢病灶的终章——探测链+诊断脚手架已拆，git 历史可查）。

## DejaVuSansMono.ttf

- 来源：DejaVu Fonts（https://dejavu-fonts.github.io/），host 路径
  `/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf`
- 许可：Bitstream Vera / 公共领域式自由许可，允许嵌入再分发
- 用途：开源占位主字体（等宽兜底，BAR-003）。已知缺口：无 CJK/盲文

## FusionPixelMono12-gb2312.ttf

- 来源：缝合像素字体（https://github.com/TakWolf/fusion-pixel-font），
  12px 等宽 zh_hans 变体，release 2026.08.11
- 许可：SIL OFL 1.1（见 OFL-fusion-pixel.txt），允许嵌入再分发
- 烘焙：`scripts/font-bake.py --subset --borrow`（GB2312 子集化 + 终端符号
  补丁表 + 借字形，6.8MB → 1.46MB；
  汉字覆盖 6618/6763，缺的 145 个为二级罕用字，渲染落 tofu）
- 借字形（BAR-027）：agnoster/omz 要而 FusionPixel 没有的 7 个符号
  （⚡✓✗✘✚➜➦）从 DejaVuSansMono 借入，upm 缩放后半角格居中
- 用途：开源占位 CJK 备用字体（像素风，与商业覆盖字体气质一致）

## local/（不进库）

用户的商业像素字体（AaHMKJXST）经 `font-bake.py --subset --monoify`
烘焙：GB2312 子集（1.86MB）+ 半角等宽化（步进钉 500、墨迹居中、
lsb=真实 xMin、超宽字形 XY 等比缩放）。等宽化后中英通吃，
同时充当主字体与 CJK 字体。
