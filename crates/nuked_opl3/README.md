# nuked_opl3

A pure-Rust port of [Nuked-OPL3](https://github.com/nukeykt/Nuked-OPL3) 1.8,
the cycle-accurate Yamaha YMF262 (OPL3) FM synthesizer emulator.

The original C implementation is Copyright (C) 2013-2020 Nuke.YKT. All of the
emulation logic and tables here are their work; this crate only translates it
to Rust.

The port exists so the emulator builds on every Rust target, including
`wasm32`, without a C toolchain. It is bit-exact with the C original: the
same register writes give the same 16-bit samples.

Every routine carries a `// = OPL3_...` comment naming the C function it
mirrors. The C original links slots and channels with raw pointers; the port
uses indices into the chip's `slot` and `channel` arrays and a small
`ModSource` enum where the C code pointed at "one of a few `i16` fields".

Stereo-extension mode (`OPL_ENABLE_STEREOEXT`) is not ported; it is off by
default in the C source.

Licensed under the GNU Lesser General Public License, version 2.1 or later,
the same license as the original. `LICENSE` is a verbatim copy of the upstream
license file.
