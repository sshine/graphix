# Terminal graphics protocols

A survey of how modern terminal emulators transport real pixels, as background for
experiments beyond the Unicode cell grid that `graphix` draws on today.

## The three protocols

- **Sixel** (DEC, 1980s): pixel data encoded as printable characters. Paletted, bulky, no
  placement control, but by far the widest support.
- **iTerm2 inline images** (OSC 1337): base64 of a whole image file. Trivially simple, full
  color, fewer bytes than sixel, no compositing.
- **Kitty graphics protocol** (APC): base64 PNG/RGB/RGBA, or transfer by file or shared
  memory. 32-bit color, image IDs with multiple placements, z-index against text, cropping,
  scrolling behavior, animation, and deletion. It is the one everyone new implements.

## Per terminal

- **Kitty** wrote the protocol and keeps extending it. Also the author of the kitty keyboard
  protocol, and of the text sizing protocol (0.40.0, March 2025) that lets programs ask for
  scaled or fractional-size text with explicit grapheme widths. Its Unicode placeholder
  extension is what makes images survive tmux.
- **Ghostty** has supported kitty graphics since 1.0, the most complete implementation outside
  kitty itself, including Unicode placeholders. Separately, custom GLSL shaders over the
  rendered grid (CRT, glow, whatever). libghostty now exposes kitty graphics to embedders, so
  the protocol reaches apps that embed a terminal rather than run one.
- **Alacritty** is not part of the graphics story, despite its reputation for modernity. It
  leads on GPU-accelerated text rendering and a deliberately minimal feature set, and has
  refused image support for years: issue #910 has been open since 2017 and the implementation
  in PR #4763 was never merged. The only working option is ayosec's fork, which carries sixel
  and iTerm2 and was still being released in April 2026. Alacritty does implement the kitty
  *keyboard* protocol, which is probably where the impression comes from.
- **WezTerm** is the only one that speaks all three protocols, which makes it the easiest
  compatibility target.
- **Konsole** has sixel and kitty graphics. **iTerm2** has its own protocol plus kitty graphics
  in 3.6+. **foot** has sixel, very fast, Wayland. **Contour** has sixel with unusually rigorous
  VT emulation. **Rio** has sixel and iTerm2. **xterm**, **mlterm**, **mintty**, **Windows
  Terminal** and **GNOME Terminal** have sixel to varying degrees.

## For a next experiment

Target kitty graphics first and sixel as the fallback; feature-detect by sending the kitty
query escape alongside a DA1 request and seeing which answer comes back.

This changes the problem from what `graphix` does now: with real pixel transport the cell grid
stops being the unit of composition and actual resolution is available, so the interesting part
becomes placement, z-ordering against text, and animation rather than glyph selection.

## Sources

- [kitty graphics protocol](https://terminfo.dev/extensions/kitty-graphics-protocol)
- [kitty text sizing protocol](https://github.com/kovidgoyal/kitty/blob/master/docs/text-sizing-protocol.rst)
- [kitty keyboard protocol](https://terminfo.dev/extensions/kitty-keyboard-protocol)
- [Ghostty features](https://ghostty.org/docs/features)
- [WezTerm features](https://wezterm.org/features.html)
- [Alacritty issue #910](https://github.com/alacritty/alacritty/issues/910)
- [Alacritty PR #4763](https://github.com/alacritty/alacritty/pull/4763)
- [ayosec/alacritty fork](https://github.com/ayosec/alacritty)
- [Are We Sixel Yet?](https://www.arewesixelyet.com/)
- [Terminal Trove comparison](https://terminaltrove.com/compare/terminals/)
- [supports-terminal-graphics](https://github.com/sindresorhus/supports-terminal-graphics)
