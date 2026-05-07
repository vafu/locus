# User systemd units

These templates assume Locus binaries were installed with `cargo install`, so
executables live under `%h/.cargo/bin`.

Install:

```sh
mkdir -p ~/.config/systemd/user
cp contrib/systemd/user/locusd.service ~/.config/systemd/user/
cp contrib/systemd/user/locus-niri.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now locusd.service locus-niri.service
```

`locusd.service` loads `%h/.config/locus/schema.yaml`. If you keep the schema
somewhere else, edit the `ExecStart=` line after copying the template.
