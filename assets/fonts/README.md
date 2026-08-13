# 内置字体

## DejaVuSansMono.ttf

- 来源：DejaVu Fonts（https://dejavu-fonts.github.io/），host 路径
  `/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf`
- 许可：Bitstream Vera / 公共领域式自由许可，允许嵌入再分发
- 用途：终端等宽兜底字体（BAR-003）。真机 Roboto 是比例字体，
  NotoSansCJK.ttc 在 fontdue 下光栅全空（BAR-002）——编译期内嵌这份
  等宽字体，任何设备都保证有及格终端字形
- 选型理由：等宽、字形完整（西文/符号/框线）、fontdue 实测可光栅、
  体积 343KB 在包体预算内（尖刺验收 <10MB）
- 已知缺口：无 CJK 字形（中文 tofu），CJK 是独立的 fallback 链切片
