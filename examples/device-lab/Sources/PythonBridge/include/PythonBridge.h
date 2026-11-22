// Device Lab's bridge to the embedded CPython (Python Lab). A small C API,
// so Swift never imports Python's own headers.
#ifndef DEVICELAB_PYTHON_BRIDGE_H
#define DEVICELAB_PYTHON_BRIDGE_H

#ifdef __cplusplus
extern "C" {
#endif

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

/// Stops the script pybridge_run is running (raises KeyboardInterrupt in
/// it at its next Python instruction). Does nothing when none is running.
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
