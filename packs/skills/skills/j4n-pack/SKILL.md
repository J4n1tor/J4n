---
name: j4n-pack
when: pack, packs, manifest, pack.toml, page, view, setting, kind, phrase, *.toml, kind:write
where: crates/j4n-core/Cargo.toml
cost: 260
---
# Write or change a pack

A pack is a folder of data: `pack.toml` (id, version, summary, needs) and TOML files of entries (`[[page]]`, `[[view]]`, `[[setting]]`, `[[command]]`, `[[rule]]`). It holds no code.

- Start with `j4n pack new <name>`; check with `j4n pack check <folder>`; it names the file and line of each problem.
- An entry's `id` is shared by every pack, so give yours a name of its own. A kind file holds `api = 1` and arrays of tables named for kinds.
- A page names a `view`; a view reads a source (`fold:<id>`, `log:<kind>`, `table:<kind>`) and says its cells.
- A setting names its type (`bool`, `int`, `num`, `str`, `dur`) and a default; a layer above the pack gives its `value`.
- Folders named `skills`, `personas` and `prompts` hold text that a model reads, not entries.
- Embedded packs are listed in `crates/j4n/src/pack.rs`; add each new file there.
