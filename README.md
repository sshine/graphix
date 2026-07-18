# graphix

Render PNG images as 24-bit ANSI block art in the terminal.

`graphix` takes a PNG input image and produces artwork made of 24-bit ANSI
colors and the shading blocks `░▒▓█`, sized to fit the current terminal.

```sh
graphix image.png             # fit to the current terminal size
graphix image.png -w 80       # constrain to 80 columns
graphix image.png -w 80 -H 24 # constrain to 80x24 cells
```

Each terminal cell covers a rectangular region of source pixels. The region
is split into a dark and a light cluster by mean luminance; the dark cluster
becomes the ANSI background color, the light cluster the foreground color,
and the shading block is chosen so its foreground coverage (`░` 25%, `▒` 50%,
`▓` 75%, `█` 100%) approximates the light cluster's share of the region.
