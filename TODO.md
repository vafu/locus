# TODO

## Hooks

- Hooks are external scripts launched by the user environment. Locus itself only
  exposes generic graph links, properties, and watches.
- The current project icon hook lives in dot-config under
  `~/.config/scripts/autorun/locus-project-icon-hook`.
- `pick-icon` depends on `~/proj/icon-picker` having:
  - `model/model.onnx`, from the ONNX export of `BAAI/bge-small-en-v1.5`
  - generated `data/icons.json`
  - generated `data/embeddings.bin`
- Generate icon-picker data with:

```sh
cd ~/proj/icon-picker
cargo run --release --bin generate-embeddings
```

Future work:

- Keep hook execution outside `locusd` so graph writes stay fast and reliable.
