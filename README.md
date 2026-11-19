# r

One short command for every task runner. `r` looks at the files in your
project (`Makefile`, `package.json`, `justfile`, ...), finds out which tool
defines the task you asked for, and runs it.

```console
$ r
make (Makefile)
  build*      Build the binary
  deploy      Ship it

yarn (package.json)
  build*      tsc
  start       node dist/index.js
  build:prod  tsc -p tsconfig.prod.json

* defined by several runners, name one: r make build

$ r start --port 3000
$ yarn run start --port 3000
...

$ r build
error: `build` is ambiguous, 2 runners define it:
  make  Makefile
  yarn  package.json
pick one with `r make build` or `r yarn build`

$ r make build
$ make build
...
```

## Install

```sh
cargo install --path .
```

## Usage

```
r [OPTIONS] [RUNNER] <TASK> [ARGS]...
r [OPTIONS] <COMMAND>
```

| Command | What it does |
| --- | --- |
| `r` / `r list` / `r ls` | List all tasks, grouped by runner. `r list npm` shows one runner, `--json` prints JSON. |
| `r <task> [args]` | Run the task with the runner that defines it. |
| `r <runner> <task> [args]` | Run the task with that runner. Needed when several runners define it. |
| `r <runner>` | List the tasks of one runner. |
| `r run [runner] <task> [args]` | Same as above, for tasks named like a built-in (`r run list`). |
| `r which [runner] <task> [args]` | Print the command that would run, without running it. |
| `r runners` | Show the detected runners and their files. |
| `r completions <shell>` | Print the completion script for `bash`, `zsh` or `fish`. |

Options, which go in front of the task or command:

- `-C, --dir <DIR>`: look for runner files in `DIR` instead of the current directory
- `-q, --quiet`: do not print the command before running it

How a task is resolved:

1. Everything after the task name is passed on untouched, including `--`:
   `r test -- --nocapture` runs `cargo test -- --nocapture`.
2. A task defined by exactly one runner just runs.
3. A task defined by more than one runner is an error (exit code 2) until you
   name the runner: `r npm start`.
4. Built-in subcommands win over tasks with the same name. Use `r run <task>`
   or `r <runner> <task>` for those; `r list` marks them with `+`.

The task runs in the directory its file was found in, and `r` exits with the
exit code of the tool.

## Supported runners

| Runner | Detected from | Runs |
| --- | --- | --- |
| `make` | `GNUmakefile`, `makefile`, `Makefile` | `make <task> [args]` |
| `npm` | `package.json` | `npm run <task> -- [args]` |
| `yarn` | `package.json` | `yarn run <task> [args]` |
| `pnpm` | `package.json` | `pnpm run <task> [args]` |
| `bun` | `package.json` | `bun run <task> [args]` |
| `just` | `justfile`, `Justfile`, `.justfile` | `just <task> [args]` |
| `task` | `Taskfile.yml`, `Taskfile.yaml` and their `.dist` variants | `task <task> -- [args]` |
| `deno` | `deno.json`, `deno.jsonc` | `deno task <task> [args]` |
| `cargo` | `Cargo.toml` | `cargo <task> [args]` |

Notes:

- **Project directory.** `r` uses the nearest directory, starting at the
  current one and walking up, that contains any of these files. It does not
  look beyond the root of the git repository.
- **package.json.** The package manager is taken from the `packageManager`
  field, then from the nearest lock file (`bun.lock`, `pnpm-lock.yaml`,
  `yarn.lock`, `package-lock.json`), and defaults to npm. You can always force
  one: `r npm start` uses npm even in a yarn project.
- **cargo.** A `Cargo.toml` has no tasks of its own, so `r` offers `build`,
  `check`, `test`, `run`, `bench`, `doc`, `clippy`, `fmt` and `clean`, plus
  the aliases from `.cargo/config.toml`.
- **Descriptions** come from `target: ## text` comments in a Makefile, the
  comment above a `just` recipe, `desc` in a Taskfile, and the command itself
  for `package.json` and `deno.json`.
- Files are parsed by `r` itself, without starting the tool, which keeps
  listing and completion fast. The price: tasks pulled in through `include`
  (make, Taskfile) or `import`/`mod` (just) are not seen, nor are make targets
  generated from variables or pattern rules.

## Shell completion

Completion is dynamic: it offers the tasks of the project you are in, the
detected runners, and after `r <runner> ` only that runner's tasks. Once a
task is typed, the shell's normal file completion takes over.

**bash**, in `~/.bashrc`:

```sh
eval "$(r completions bash)"
```

**zsh**, in `~/.zshrc` after `compinit`:

```sh
disable r
source <(command r completions zsh)
```

`r` is a zsh builtin that repeats the last command, and it shadows the binary.
`disable r` turns the builtin off; without it zsh never calls this program.

**fish**:

```sh
r completions fish > ~/.config/fish/completions/r.fish
```

## Development

```sh
cargo test      # unit tests and end-to-end tests with fake tools
cargo clippy --all-targets
```

Each runner lives in its own file in `src/runner/`. To add one, write a
`detect` function there, register it in `detect_all` in `src/runner/mod.rs`
and add its command line to `Runner::command`.
