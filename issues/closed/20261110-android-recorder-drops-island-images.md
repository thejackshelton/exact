# The Android recorder draws no text shadow, text stroke, clipped-text fill or filtered SVG island

**Status:** Closed
**Resolution:** The recorder places an island as a picture, and no painting method of `Backend` has a default body (LLP 1159.000 D1); a blended island draws unblended and says so. Not seen on a device.
**Systems:** host/linux/src/canvas.rs (`canvas::Recorder`), host/linux/src/paint/backend.rs
**Severity:** P2
**Author:** Claude (Fable 5.2), from the 2026-11-10 project review
**Date:** 2026-11-10

## What is wrong

Line references are at commit `dd6f5dcd`.

The painter hands some results to its backend as finished pixels through `Backend::island_image`. The trait gives that method a default body that does nothing (`host/linux/src/paint/backend.rs:203`):

```rust
fn island_image(&mut self, _pixels: Arc<Pixmap>, _dst: Rect4, _ts: Transform, _mode: u8) {}
```

The CPU, GPU and Ink backends implement it (`raster.rs:907`, `gpu.rs:810`, `ink.rs:571`). The Canvas recorder, which writes the stream Android's reader plays, does not, so everything painted that way is computed and then dropped. The callers are:

- a text stroke (`paint/text_stroke.rs:52`);
- a text shadow (`paint/text_shadow.rs:194`);
- `background-clip: text` (`paint/text_clip.rs:81`);
- an SVG filter's island (`paint/island.rs:166`) and one more SVG path (`paint/svg.rs:550`).

## The failure

```
text "Shadowed" text-shadow="2px 2px 4px #000000"
text "Outlined" -webkit-text-stroke="2px #000000"
```

On Linux and Windows both draw. On Android the first has no shadow and the second no outline, and the paint still spends the CPU time making the pixels.

## Verification

Ran the painter over the real `canvas::Recorder` for a paragraph with `-webkit-text-stroke="2px #000000"`, one with `text-shadow="2px 2px 4px #000000"` and a plain one: the three finished streams are the same 100 words and three ops (matrix, font, glyphs). Read, not run: the `background-clip: text` and SVG callers, and the Kotlin reader, which has no other route for these pixels that this review found. Not run on a device.

## Direction

Give the recorder an `island_image` that writes the pixels as a picture op, as its `surface_image` does for a masked SVG item, and remove the default body so that a backend without one does not compile. LLP 1001, LLP 1077 and QUEUE.md declare no such gap.

## Related

`issues/20261110-text-stroke-cost-grows-with-width-squared.md` measures the cost of the stroke's island on every painter. `issues/20261110-datasource-wrappers-lose-calls-to-trait-defaults.md` is the same shape in another trait: a default body that hides a missing implementation.
