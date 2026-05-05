# TODO

## Hooks

- `locushookd` is currently a separate client daemon that listens to `locusd`
  D-Bus graph signals.
- The first hook is hardcoded: when a project is registered and has no `icon`
  property, `locushookd` runs `pick-icon` from `PATH` and writes the selected
  icon back to the project as an `icon` property.
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

- Make hooks configurable instead of hardcoded in `locushookd`.
- Keep hook execution outside `locusd` so graph writes stay fast and reliable.
- Add more built-in hooks first if that is simpler than designing the config
  format up front.
