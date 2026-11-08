# forge-crash test fixtures

- `crashy.c`: a tiny program that traps in `trap_here`, called from `depth`, called from `main`.
- `Crashy.dSYM`: its dSYM. Built on Linux with LLVM, no Apple tools:

  ```bash
  clang -target arm64-apple-ios17.0 -g -O0 -c crashy.c -o crashy.o
  ld64.lld -arch arm64 -platform_version ios 17.0 17.0 -e _main -o Crashy crashy.o
  dsymutil Crashy -o Crashy.dSYM   # UUID 4c4c4451-5555-3144-a1b5-b63c7a520c29
  ```

  Rebuilding changes the UUID: update `Crashy-2026-11-08-101500.ips` to match.
- `Crashy-2026-11-08-101500.ips`: a crash report for that build, in the iOS 26 `.ips` layout (hand-written; offsets from `llvm-objdump --macho -d Crashy`).
- `ChatXtool-2026-11-08-093000.ips`: a Rust panic on a Tokio worker thread with the phone's own (mangled) symbols and no local binary, as when the build that crashed is gone.
