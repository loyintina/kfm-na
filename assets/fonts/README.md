# 内置字体

## 编译期选择机制（BAR-021，2026-08-18）

`build.rs` 编译期二选一，源码树零生成物：

- `local/main.ttf` 存在 → 主字体用它（**本机商业字体，gitignore
  钉死永不进库**；chain.sh 第 1 步防泄漏闸机械执法）
- 主字体占位 = DejaVuSansMono.ttf；CJK/符号 fallback 恒定 =
  NotoSansCJKsc-kfm.ttf（BAR-022：商业美术字体天然缺终端符号，
  fallback 的职责就是补盲文/方块/几何符号——主字体缺的字形由
  prefer_cjk 逐字路由给它；BAR-176：fallback 的汉字覆盖升全量黑体）

生产启动**零探测**：不读 /system/fonts，TermView 毫秒级建成
（BAR-020 启动慢病灶的终章——探测链+诊断脚手架已拆，git 历史可查）。

## DejaVuSansMono.ttf

- 来源：DejaVu Fonts（https://dejavu-fonts.github.io/），host 路径
  `/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf`
- 许可：Bitstream Vera / 公共领域式自由许可，允许嵌入再分发
- 用途：开源占位主字体（等宽兜底，BAR-003）。已知缺口：无 CJK/盲文

## NotoSansCJKsc-kfm.ttf（CJK 备用，BAR-176，2026-09-29）

- 来源：Noto Sans CJK SC Regular（https://github.com/notofonts/noto-cjk），
  SIL OFL 1.1（见 OFL-noto.txt），允许嵌入再分发
- 为什么换：旧备用（缝合像素 GB2312 子集）超集汉字主备双缺——用户报障
  「涅槃的槃」终端纯黑不可见。用户拍板：CJK 备用换尽量全的黑体，
  **像素气质让位于覆盖率**。像素时代就此结业（FusionPixel 全链退库，
  git 历史可查）
- 烘焙：`scripts/font-bake-cjk.py`（四道工序：subset → otf2ttf → 归格 →
  借形；用法见脚本 docstring）。保留码位：BMP 汉字全量（URO U+4E00-9FFF
  + 扩展 A U+3400-4DBF + 兼容表意 U+F900-FAFF，实测 27924）+ GB2312 全表
  + ASCII + 终端符号补丁表；**Ext B+（SIP）从第一刀不保**（体积账：
  产物 8.96MB，进 git 需 chain.sh 第 1 步 16MB 豁免）
- otf2ttf：Noto 是 CFF 轮廓，借形/归格机械要 glyf 表——cu2qu 转 TrueType
- 归格（终端按 unicode-width 摆格）：宽字符（EAW W/F）保 1em 全角位；
  窄字符 advance 归半格、墨迹居中（Noto 原生 ASCII 'M' advance 812/1000，
  不归格占 1.6 格）；框线/方块区只做 X 向压半格（XY 等比会把线宽压没，
  tmux 边框变虚线）；格式控制符不动
- 借形补缺（「源字体已有码点跳过」守卫 = 黑体气质优先）：箭头/几何/
  盲文/powerline/月亮 🌑-🌘/替换符 FFFD 捐体 FusionPixel（已退库，
  重烘前先 `git show` 取回，脚本 FP_DONOR 环境变量可指）；
  Latin-1/通用标点/货币补漏捐体 DejaVuSansMono→DejaVuSans；
  emoji 23 个捐体 NotoEmoji（构建期依赖不进库）；
  **带圈全族 62 码位（Ⓐ-ⓩ U+24B6-24E9 + ⑪-⑳ U+246A-2473）捐体
  NotoSansSymbols（BAR-242 翻案重开，2026-10-14：用户裁定补字体
  不接受降级；捐体 = Noto Sans Symbols Regular，SIL OFL 同族许可，
  构建期依赖不进库——脚本 NSS_DONOR 环境变量可指，默认
  ~/40-资产/kfm-na-toolchain/fonts/NotoSansSymbols-Regular.ttf）。
  **全角归格（cells=2，同日二修）**：半格版（480 墨盒 = 17px 圆、圈内
  字母 ~7px）被用户真机判「太小、几乎看不到」——带圈族按 CJK 惯例
  全角借入（824 墨盒零缩放 ≈「中」90% 高），UI 文本路径 char_cells
  同族判 2 格（BAR-199 箭头块同型分叉），终端格路径（alacritty 宽度表
  判 1 格）放不进全角 → degrade_cell 落单个 ASCII 字母。①-⑩ 未并轨
  （存量八千处零报障，并轨与否另裁）。补丁于在产烘焙品直补（借形幂等，
  脚本已改好未来重烘自带该族）**；
  **戊窄版孪生（2026-10-16，BAR-242 三开——NA0171 楼20 用户终验未过：
  终端面读信仍见降级 a/b；楼26 判做「同尺寸重试」）**：带圈族另绘
  **窄版孪生**进 PUA U+E000-E03D（与主码位同序一一映射，font-bake-cjk.py
  `twin_cp`），**重绘非缩绘**——环粗 60 墨盒＋字母墨高 ≈55% 圆径
  （字母捐体 DejaVuSans-Bold；一修缩绘版环 38/字母 40% 是「太小」真因）。
  主码位全角字形不动（UI 面二修口径不破）；孪生只被 termview
  degrade_cell 条件②消费（孪生在场换孪生、缺席才降级单字母——降级路
  不退役）。直补 `scripts/font-patch-enclosed-twin.py`（与烘焙管线同一
  函数，重烘自带）；**边界：试制件——用户上机过眼前不改判据不结案**
- 缺口边界（双源皆无，不补）：杂项符号 52 个、装饰符号 141 个、
  powerline 私有区 17 个（E0B3-E0BB/E0CC-E0CF 连体字形族）
- 判卷（脚本尾自打印+断言）：槃/中/🌑/⠋/░/─/✅/E0B0/⚡/FFFD cmap+墨迹、
  汉字 ≥27900、ASCII 步进全半格、月亮全角位、powerline 墨迹贴满半格
  左右缘、体积 ≤15MB；BAR-242 增 ⓐⓏⒶⓩ⑪⑳ 六探针 cmap+墨迹+带圈全族
  步进=全角断言（二修：用户判半格太小）

## 退役：FusionPixelMono12-gb2312.ttf（2026-09-20 入库 → BAR-176 退库）

像素风 CJK 备用，GB2312 子集（汉字 6618/6763）+ 终端符号补丁全家桶
（BAR-022/027/032/113 + 月亮/FFFD 补丁）。符号补丁最全，是 BAR-176 烘焙
的头号捐体；汉字覆盖率是死穴（超 GB2312 纯黑），被 Noto 大字库接替。
历史烘焙管线 `scripts/font-bake.py` 保留（商业 local 字体仍在用）。

## local/（不进库）

用户的商业像素字体（AaHMKJXST）经 `font-bake.py --subset --monoify`
烘焙：GB2312 子集（1.86MB）+ 半角等宽化（步进钉 500、墨迹居中、
lsb=真实 xMin、超宽字形 XY 等比缩放）。等宽化后中英通吃，
同时充当主字体与 CJK 字体。
