# Argon2 (third-party component)

Bloom embeds the **reference implementation of Argon2** for
`std::auth` password hashing.

- Version: **20190702** (last tagged release)
- Upstream: <https://github.com/P-H-C/phc-winner-argon2>
- Source archive: `https://github.com/P-H-C/phc-winner-argon2/archive/refs/tags/20190702.zip`
- Files used (unmodified): `runtime/argon2/include/argon2.h`,
  `runtime/argon2/src/{argon2.c,core.c,core.h,encoding.c,encoding.h,ref.c,thread.c,thread.h}`,
  `runtime/argon2/src/blake2/{blake2.h,blake2b.c,blake2-impl.h,blamka-round-ref.h}`,
  `runtime/argon2/LICENSE`.
- Not vendored: CLI tools, benchmarks, KAT generator, VS projects.

Argon2 is distributed under **CC0 1.0 Universal or Apache License 2.0**
(at your option). The full license text is in `runtime/argon2/LICENSE`;
the headers of every source file carry the same notice:

> Copyright 2015 Daniel Dinu, Dmitry Khovratovich, Jean-Philippe Aumasson,
> and Samuel Neves. You may use this work under the terms of a Creative
> Commons CC0 1.0 License/Waiver or the Apache Public License 2.0, at your
> option.

Bloom compiles the implementation with `ARGON2_NO_THREADS` (Bloom has no
threads; the parallelism parameter of the hashes is 1) and uses the plain
reference code (`ref.c`), not the optional SIMD variants.

The compiler writes the Argon2 tree into the temporary build directory
only for programs that load `std::auth`. `bloom build --emit-c` copies the
same tree next to the emitted C file.
