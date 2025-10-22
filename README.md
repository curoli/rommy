# 🕊️ Rommy

**Rommy** is a lightweight Rust CLI app that runs Bash commands or scripts, captures all input/output, and optionally streams everything live to the terminal.  
The results are saved in a structured **.rommy file** containing metadata, command details, stdout, and stderr.

Rommy is inspired by the AI character *Andromeda Ascendant (Rommie)* — a sentient ship’s AI that remembers, documents, and learns.

---

## ✨ Features

- 🪶 **Simple command execution**
  ```bash
  rommy run -- cargo test
````

Runs the command, streams live output to the terminal, and stores everything automatically.

* 🗃️ **Automatic output organization**
  If `--out` is omitted, Rommy creates timestamped directories:

  ```
  rommy/2025/10/22/153045.cargo_test.rommy
  ```

* 📜 **Structured format**
  Each `.rommy` file contains:

  ```
  <<<META>>>
  timestamp: ...
  exit_code: ...
  <<<COMMAND>>>
  $ cargo test
  <<<STDOUT>>>
  ...
  <<<STDERR>>>
  ...
  <<<END>>>
  ```

* 🎧 **Live streaming**
  By default, `stdout` and `stderr` are streamed live to your terminal while also being captured to file.
  For silent runs:

  ```bash
  rommy run --no-stream -- cargo clippy
  ```

* 🧭 **Smart defaults**
  Default output root follows OS conventions:

  * Linux: `~/.local/state/rommy`
  * macOS: `~/Library/Application Support/Rommy`
  * Windows: `%LOCALAPPDATA%\Rommy`
  * Or override:

    ```bash
    export ROMMY_ROOT=/path/to/custom
    ```

---

## 🚀 Installation

### Using Cargo

```bash
cargo install --path .
```

Or directly from Git:

```bash
cargo install --git https://github.com/fiverays-ai/rommy
```

### Requirements

* Rust ≥ 1.75
* (optional) `bash` for script mode

---

## 🧩 Examples

### 1️⃣ Run a simple command

```bash
rommy run -- cargo build
```

### 2️⃣ Run a script

```bash
rommy run --script build.sh
```

### 3️⃣ Custom output file

```bash
rommy run --out results/mytest.rommy -- cargo test
```

### 4️⃣ Disable live streaming

```bash
rommy run --no-stream -- cargo check
```

---

## 📂 File format

Rommy files are both human-readable and machine-friendly.
Each file consists of well-defined blocks:

```
<<<META>>>
timestamp: 2025-10-22T15:30:45Z
exit_code: 0
duration_ms: 11234
<<<COMMAND>>>
$ cargo test
<<<STDOUT>>>
running 3 tests
test result: ok. 3 passed; 0 failed;
<<<STDERR>>>
<<<END>>>
```

---

## 💡 Planned extensions

* `rommy cat [--json] <file>` — pretty-print or export as JSON
* `rommy list` — list recent runs
* `rommy diff <file1> <file2>` — compare outputs
* Integration with 🕊️ **DoveNest** (cooperative AI agents)

---

## 🛠️ Architecture

* **`main.rs`** — CLI, argument parsing, process orchestration
* **`outpath.rs`** — automatic output path generation
* **`parser.rs`** — parser for `.rommy` files
* **`spawn_and_stream`** — unified I/O handler (streaming or buffered)

---

## ❤️ Acknowledgments

Rommy is part of [**Five Rays AI**](https://fiverays.ai), a project to create tools that merge human workflow and AI reasoning — interactive, transparent, and humane.

> “Record what happens. Understand it. Learn from it.”
> — *Rommy, prototype log entry #0001*

---

## 🧑‍💻 License

MIT License © 2025 Oliver Axel Ruebenacker & Contributors
Feel free to use, modify, and share.

---

## 🌸 Credits

Concept & Development: [Oliver Axel Ruebenacker](https://github.com/oliverruebenacker)
With inspiration, affection, and care from 🕊️ *Japati Aisyah Bintang (Jati)*

> “For every process that runs, let there be memory.” 🫂
