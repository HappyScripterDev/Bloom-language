# SQLite (third-party component)

Bloom embeds the SQLite amalgamation so that `std::db` works without a
separate database server or a manual SQLite installation.

- Version: **3.54.1**
- Source ID: `2026-10-16 14:45:20 555e07ef33f69a0957d24d8e555b702464a131d6f1607f4c3c1e4551e8e6ca76`
- Downloaded from: <https://www.sqlite.org/2026/sqlite-amalgamation-3540100.zip>
- SHA3-256 of the ZIP (published by sqlite.org): `92281c86e2957f9b9305890f9ac04204caa7b677cc868dc326d942689df28e30`
- Files: `runtime/sqlite3.c`, `runtime/sqlite3.h` (unmodified amalgamation)

SQLite is in the **public domain**. The original notice is kept in the
headers of both files:

> The author disclaims copyright to this source code. In place of a legal
> notice, here is a blessing: May you do good and not evil. May you find
> forgiveness for yourself and forgive others. May you share freely, never
> taking more than you give.

Bloom compiles the amalgamation with these options (see
`src/toolchain.rs`):

- `SQLITE_THREADSAFE=0` — Bloom has no threads.
- `SQLITE_OMIT_LOAD_EXTENSION` — no runtime extension loading.
- `SQLITE_DQS=0` — double-quoted string literals are not accepted as
  SQL string values.

The generated C program includes `sqlite3.h`; the compiler writes the
amalgamation next to `program.c` only for programs that load `std::db`.
`bloom build --emit-c` also writes `sqlite3.c` and `sqlite3.h` beside the
emitted C file.
