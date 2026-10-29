# Red Hat AI Configurator

A terminal installer that configures the [goose](https://github.com/aaif-goose/goose)
AI agent for two model services:

| Provider | Internal name | goose engine |
|---|---|---|
| Red Hat AI MaaS | `rhai-maas` | OpenAI compatible |
| EnMaaS | `enmaas` | Anthropic compatible, plus an OpenAI-compatible provider for Gemini |

It is a single static Rust binary, packaged as an RPM, with a ratatui TUI for
people and a non-interactive mode for Ansible and image builds.

Service endpoints are **not embedded**. A public build ships placeholders; set
them for your deployment with environment variables or
`/etc/redhat-ai-configurator/endpoints.conf` (see below).

## What it does

1. Checks the host: RHEL version, `goose` on `PATH` (1.38 or newer), the
   configuration directory, network reachability of both services, and the
   current VPN state.
2. Lets you choose Red Hat AI MaaS, EnMaaS, or both. Both are selected by
   default.
3. Shows the configured key page for each provider and asks you to paste a key.
   It **validates the key against `/models` before writing anything**, and
   retries with the other auth header if the first is rejected.
4. Lets you pick the default model and goose's tool mode.
5. Writes declarative provider files and merges the model settings into
   `config.yaml`, backing up every file it replaces.
6. Runs `goose info --check` and reports the result.

It never creates API keys, never opens a browser, never reads the clipboard,
and never logs a key. Obtain the keys yourself at the URLs shown in the UI.

### VPN requirements

The reference deployment expects Red Hat AI MaaS with the corporate VPN **off**
and EnMaaS with it **on**. The installer shows the required state at each step
and lets you re-check a provider without losing what you typed, so you can do
one provider, change VPN state, and do the other.

## Requirements

- RHEL 10.2 (x86_64 or aarch64)
- The RHEL extensions repository, which provides `goose`:

  ```sh
  sudo subscription-manager repos --enable=rhel-10-for-$(arch)-extensions-rpms
  ```

- `goose >= 1.38` (the RPM declares this dependency)
- Optional: `libsecret-tools` if you want to store keys in the system keyring
  instead of `secrets.yaml`

## Configure the endpoints

Resolution order: environment, then `/etc/redhat-ai-configurator/endpoints.conf`,
then the placeholder defaults. Environment wins.

| Variable | Meaning | Default |
|---|---|---|
| `RHAI_MAAS_BASE_URL` | OpenAI-compatible base, including `/v1` | `https://maas.example.com/v1` |
| `RHAI_MAAS_KEY_PAGE` | Page where users create a key | `https://maas.example.com/keys` |
| `RHAI_MAAS_HELP_URL` | Help link shown in the UI | `https://maas.example.com/docs` |
| `ENMAAS_BASE_URL` | Anthropic-compatible root, **without** `/v1` | `https://enmaas.example.com` |
| `ENMAAS_KEY_PAGE` | Page where users create a key | `https://enmaas.example.com/keys` |
| `ENMAAS_HELP_URL` | Help link shown in the UI | `https://enmaas.example.com/docs` |
| `ENMAAS_ELIGIBILITY_URL` | Access-request page (optional) | unset |
| `ENMAAS_SLACK` | Help channel shown in the UI (optional) | unset |

The same keys work in the config file:

```ini
# /etc/redhat-ai-configurator/endpoints.conf
RHAI_MAAS_BASE_URL=https://maas.example.com/v1
ENMAAS_BASE_URL=https://enmaas.example.com
```

When the bases are still placeholders the UI and `--doctor` warn, and the
non-interactive report includes a warning.

## Install

```sh
sudo dnf install ./redhat-ai-configurator-0.1.0-1.el10.x86_64.rpm
```

Then run it **as yourself**, not as root:

```sh
redhat-ai-configurator
```

## Interactive use

The TUI walks through five screens:

1. **System** — OS, user, configuration directory, `goose` version, current VPN
   state, endpoint warning if placeholders are in use, and whether each service
   is reachable.
2. **Providers** — space to toggle, enter to continue.
3. **Keys** — the configured key page, the VPN requirement, and a masked input
   per provider. Enter validates and moves to the next provider. If a key is
   already stored, enter keeps it; `ctrl+u` clears the field.
4. **Model** — default model per provider (the catalog is merged with the live
   model list), goose mode, and key storage.
5. **Review** — exactly which files will be written. Enter applies, then the
   result of `goose info --check` is shown.

`esc` goes back, `ctrl+c` quits. The alternate screen closes with a recap that
stays in scrollback, including the key page URLs.

## Non-interactive use

```sh
redhat-ai-configurator --non-interactive \
  --provider rhai-maas,enmaas \
  --key-file - \
  --mode approve \
  --format json
```

Keys come from, in order:

1. `RHAI_MAAS_API_KEY` / `ENMAAS_API_KEY`
2. `--key-file PATH` (mode 0600, owned by the invoking user)
3. `--key-file -` (stdin)

The key file is a plain env file using the same names:

```
# keys.env, chmod 600
RHAI_MAAS_API_KEY=...
ENMAAS_API_KEY=...
```

Useful flags:

| Flag | Purpose |
|---|---|
| `--dry-run` | print the planned changes, write nothing |
| `--keep-existing` | reuse stored keys; makes repeat runs idempotent |
| `--no-validate` | skip the network check (offline image builds) |
| `--require-validation` | fail instead of warning when a key cannot be validated |
| `--no-verify` | skip `goose info --check` |
| `--rollback-on-failure` | restore backups if verification fails |
| `--keyring` | store keys in the system keyring |
| `--model PROVIDER=MODEL` | choose the default model |
| `--format json` | machine-readable output |
| `--list-models` | print the service's model list and exit |
| `--doctor` | check an existing installation without changing it |

Exit codes are stable: `0` success, `2` usage, `3` goose missing/too old,
`4` key problem, `5` network/VPN, `6` write failure, `7` verification failure.

### Ansible

The module must run as the user who owns the goose configuration:

```yaml
- name: Configure goose for the AI MaaS providers
  ansible.builtin.command:
    argv:
      - /usr/bin/redhat-ai-configurator
      - --non-interactive
      - --provider
      - rhai-maas,enmaas
      - --mode
      - approve
      - --format
      - json
  environment:
    RHAI_MAAS_API_KEY: "{{ rhai_maas_key }}"
    ENMAAS_API_KEY: "{{ enmaas_key }}"
    RHAI_MAAS_BASE_URL: "{{ rhai_maas_base_url }}"
    ENMAAS_BASE_URL: "{{ enmaas_base_url }}"
  become_user: alice
  register: cfg
  changed_when: (cfg.stdout | from_json).changed
  no_log: true
```

A single run can usually validate only one provider when the two services want
opposite VPN states. The other provider is reported as `"validated": false`
with a warning; add `--require-validation` to make that a failure instead.

## What gets written

```
~/.config/goose/
├── config.yaml                         # merged, backed up
├── custom_providers/
│   ├── rhai-maas.json                  # engine: openai
│   ├── enmaas.json                     # engine: anthropic
│   └── enmaas-openai.json              # engine: openai (Gemini)
└── secrets.yaml                        # 0600, only when file storage is used
```

`config.yaml` gets both the flat keys that goose 1.38 reads and the structured
`active_provider` / `providers:` block that newer releases prefer:

```yaml
GOOSE_PROVIDER: rhai-maas
GOOSE_MODEL: publishers/prelude-maas/models/glm-53-flash
GOOSE_MODE: approve
GOOSE_DISABLE_KEYRING: true
active_provider: rhai-maas
providers:
  rhai-maas:
    enabled: true
    configured: true
    model: publishers/prelude-maas/models/glm-53-flash
```

Everything else in the file, including extensions, is preserved. Existing
files are copied to `<name>.bak-YYYYMMDD-HHMMSS` before being replaced, and a
file is only rewritten when its content actually changes, so repeated runs are
idempotent.

Every model entry declares a `context_limit`. goose 1.45 requires that field
and rejects the entire `custom_providers` directory if any single file omits
it, so the configurator always writes it (200000 for most models, 1000000 for
GLM, DeepSeek and Gemini).

## Security

- Keys are never printed, logged, or included in JSON output.
- `secrets.yaml` is mode 0600 in a 0700 directory.
- `--key-file` refuses group- or world-readable files and reports only line
  numbers for malformed content.
- The tool refuses to run as root (`--allow-root` exists for containers).
- Keyring storage uses goose's own Secret Service entry through `secret-tool`,
  and falls back to the file with a warning if the keyring is unavailable.

## Build

```sh
cargo build --release --locked
cargo test --locked
cargo clippy --all-targets -- -D warnings
```

Rust 1.88 or newer is required; RHEL 10.2's Rust 1.92 works.

### RPM

`cargo vendor` is used so the build is offline and reproducible:

```sh
make tarballs     # source and vendor tarballs in dist/
make rpm          # rpmbuild -ba, output in dist/rpmbuild/RPMS/
```

On a clean RHEL 10 host:

```sh
sudo dnf install -y cargo rust gcc make rpm-build rpmdevtools
make rpm
```

## Layout

```
src/main.rs        entry point, exit codes
src/cli.rs         flags, non-interactive flow, doctor, list-models
src/apply.rs       shared plan/commit core
src/tui.rs         ratatui screens and event loop
src/catalog.rs     providers, models, endpoint accessors
src/settings.rs    endpoint resolution (env, config file, defaults)
src/netcheck.rs    key validation, model discovery, reachability
src/goose/         config.yaml, custom providers, secrets, goose runner
src/platform.rs    OS, goose discovery, VPN detection
packaging/         RPM spec, man page, example endpoints file
tests/             end-to-end CLI tests
```

## License

MIT. See `LICENSE`.
