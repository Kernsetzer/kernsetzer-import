# kernsetzer-import

Manuscript importers of Kernsetzer, a
book-formatting app for self-publishing authors. Reads `.docx` and Markdown
into the document model of
[kernsetzer-model](https://github.com/Kernsetzer/kernsetzer-model). Odd input
produces warnings, not failures.

```rust
let imported = setzer_import::read_manuscript(std::path::Path::new("novel.docx"))?;
```

The library name is `setzer_import`. Every test fixture is taken verbatim
from public-domain text (Austen, Kafka).

## License

MIT OR Apache-2.0, at your option. The rest of Kernsetzer is not part of
this repository and is under a different license. The name "Kernsetzer" is
a trademark of Lukas Nutz and is not licensed with the code.

Issues are welcome; pull requests are not being accepted yet.
