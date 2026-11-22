// Device Lab's bridge to the embedded CPython (Python Lab). A small C API,
// so Swift never imports Python's own headers.
#ifndef DEVICELAB_PYTHON_BRIDGE_H
#define DEVICELAB_PYTHON_BRIDGE_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/// What a streaming run's callback is told (`kind`).
enum {
    /// Output the script wrote to stdout (UTF-8, `length` bytes).
    PYBRIDGE_STDOUT = 1,
    /// Output it wrote to stderr, tracebacks included.
    PYBRIDGE_STDERR = 2,
    /// input() is waiting for pybridge_send_input (empty text).
    PYBRIDGE_INPUT_WAITING = 3,
    /// input() got its line (the text, without a newline).
    PYBRIDGE_INPUT_ENDED = 4,
    /// Stop cancelled the input() that was waiting (empty text).
    PYBRIDGE_INPUT_CANCELLED = 5,
};

/// A streaming run's callback, called on the run's thread with Python's
/// GIL held: copy the text and return quickly, and never wait there for
/// anything that may itself be waiting on Python (pybridge_interrupt
/// takes the GIL). `text` isn't NUL-terminated; `length` is its size.
typedef void (*pybridge_write_fn)(void *context, int kind, const char *text, size_t length);

/// Starts the interpreter once, with `home` as PYTHONHOME (the app bundle's
/// python folder) and `site_dir` as a writable folder added to sys.path.
/// Returns NULL on success, or a malloc'd error message (free it with
/// pybridge_free). Calling it again after a success does nothing.
char *pybridge_start(const char *home, const char *site_dir);

/// Runs `code` as a script in a fresh namespace, from any thread. Output
/// (stdout and stderr, then the traceback of an exception) is returned as a
/// malloc'd UTF-8 string; free it with pybridge_free. If the last statement
/// is an expression, its repr() is printed, as in the REPL. `ok` is set to
/// 1 when the script finished without an exception, else 0.
char *pybridge_run(const char *code, int *ok);

/// Runs `code` like pybridge_run, but hands its output to `write` as it is
/// written (stdout and stderr apart, a traceback on stderr) instead of
/// returning it. With `with_input`, input() waits for pybridge_send_input;
/// without it, input() meets the end of input. Returns 1 when the script
/// finished without an exception, else 0. One run at a time.
int pybridge_run_streaming(const char *code, int with_input, pybridge_write_fn write,
                           void *context);

/// Answers the input() a streaming run is waiting for with `line` (no
/// newline). Returns 1 when one was waiting and took it, else 0.
int pybridge_send_input(const char *line);

/// Stops the script pybridge_run or pybridge_run_streaming is running
/// (raises KeyboardInterrupt in it at its next Python instruction, or in
/// the input() it waits in). Does nothing when none is running.
/// Returns 1 when a script was running and KeyboardInterrupt was scheduled
/// in it (the script may still catch it), else 0.
int pybridge_interrupt(void);

/// The interpreter's version ("3.15.0 (main, ...)"); malloc'd.
char *pybridge_version(void);

void pybridge_free(char *text);

#ifdef __cplusplus
}
#endif

#endif
