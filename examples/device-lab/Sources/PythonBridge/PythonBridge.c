// See include/PythonBridge.h. Python's C API, following CPython's own iOS
// testbed (Apple/testbed in the CPython source) for the configuration.
#include "PythonBridge.h"

#include <Python/Python.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static pthread_mutex_t lock = PTHREAD_MUTEX_INITIALIZER;
static int started = 0;
// The runner functions defined by RUNNER and STREAM_RUNNER below.
static PyObject *runner = NULL;
static PyObject *stream_runner = NULL;
// The thread running a script, for pybridge_interrupt (0 when none).
static unsigned long running_thread = 0;

// The streaming run's output callback and its context (NULL when none).
// Set and read with the GIL held.
static pybridge_write_fn stream_write = NULL;
static void *stream_context = NULL;

// input(): a streaming run with input on blocks in `devicelab_readline`
// (the GIL released) until pybridge_send_input gives it a line or
// pybridge_interrupt cancels it. All under `input_lock`.
static pthread_mutex_t input_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t input_ready = PTHREAD_COND_INITIALIZER;
static int input_enabled = 0;
static int input_waiting = 0;
static int input_cancelled = 0;
// Stop came while the script ran Python, not while it waited: an input()
// it reaches before the KeyboardInterrupt lands is cancelled too.
static int interrupt_requested = 0;
static char *input_line = NULL;

static const char *RUNNER =
    "import ast, io, sys, traceback\n"
    "def _devicelab_run(source):\n"
    "    out = io.StringIO()\n"
    "    saved = sys.stdout, sys.stderr\n"
    "    sys.stdout = sys.stderr = out\n"
    "    ok = True\n"
    "    try:\n"
    "        tree = ast.parse(source, '<script>')\n"
    "        last = None\n"
    "        if tree.body and isinstance(tree.body[-1], ast.Expr):\n"
    "            last = ast.Expression(tree.body.pop().value)\n"
    "        scope = {'__name__': '__main__'}\n"
    "        exec(compile(tree, '<script>', 'exec'), scope)\n"
    "        if last is not None:\n"
    "            value = eval(compile(last, '<script>', 'eval'), scope)\n"
    "            if value is not None:\n"
    "                print(repr(value))\n"
    "    except BaseException:\n"
    "        ok = False\n"
    "        traceback.print_exc()\n"
    "    finally:\n"
    "        sys.stdout, sys.stderr = saved\n"
    "    return ok, out.getvalue()\n";

// The streaming runner: the same as RUNNER, but stdout and stderr go to
// the host as they are written, stdin is the person's when `interactive`,
// and an exception the wrapper already showed (python_lab.rs marks it
// `_devicelab_shown`) isn't printed again.
static const char *STREAM_RUNNER =
    "import io as _io\n"
    "import _devicelab_io\n"
    "class _DevicelabOut(_io.TextIOBase):\n"
    "    def __init__(self, kind):\n"
    "        self.kind = kind\n"
    "    def writable(self):\n"
    "        return True\n"
    "    def write(self, text):\n"
    "        if not isinstance(text, str):\n"
    "            raise TypeError(f'write() argument must be str, not {type(text).__name__}')\n"
    "        _devicelab_io.write(self.kind, text)\n"
    "        return len(text)\n"
    "class _DevicelabIn(_io.TextIOBase):\n"
    "    def readable(self):\n"
    "        return True\n"
    "    def readline(self, size=-1):\n"
    "        return _devicelab_io.readline()\n"
    "def _devicelab_stream(source, interactive):\n"
    "    saved = sys.stdout, sys.stderr, sys.stdin\n"
    "    sys.stdout, sys.stderr = _DevicelabOut(1), _DevicelabOut(2)\n"
    "    sys.stdin = _DevicelabIn() if interactive else _io.StringIO('')\n"
    "    ok = True\n"
    "    try:\n"
    "        tree = ast.parse(source, '<script>')\n"
    "        last = None\n"
    "        if tree.body and isinstance(tree.body[-1], ast.Expr):\n"
    "            last = ast.Expression(tree.body.pop().value)\n"
    "        scope = {'__name__': '__main__'}\n"
    "        exec(compile(tree, '<script>', 'exec'), scope)\n"
    "        if last is not None:\n"
    "            value = eval(compile(last, '<script>', 'eval'), scope)\n"
    "            if value is not None:\n"
    "                print(repr(value))\n"
    "    except BaseException as error:\n"
    "        ok = False\n"
    "        if not getattr(error, '_devicelab_shown', False):\n"
    "            # The script's frames only, not this runner's.\n"
    "            frames = [f for f in traceback.extract_tb(error.__traceback__) if f.filename != '<string>']\n"
    "            sys.stderr.write('Traceback (most recent call last):\\n'\n"
    "                + ''.join(traceback.format_list(frames))\n"
    "                + ''.join(traceback.format_exception_only(type(error), error)))\n"
    "    finally:\n"
    "        sys.stdout, sys.stderr, sys.stdin = saved\n"
    "    return ok\n";

// Hands `text` to the streaming run's callback. Needs the GIL.
static void emit(int kind, const char *text, size_t length) {
    if (stream_write) stream_write(stream_context, kind, text, length);
}

// _devicelab_io.write(kind, text): output from the streaming run.
static PyObject *devicelab_write(PyObject *self, PyObject *args) {
    (void)self;
    int kind;
    PyObject *text;
    if (!PyArg_ParseTuple(args, "iU", &kind, &text)) return NULL;
    // Lone surrogates become "?" rather than failing the print.
    PyObject *bytes = PyUnicode_AsEncodedString(text, "utf-8", "replace");
    if (bytes == NULL) return NULL;
    emit(kind, PyBytes_AS_STRING(bytes), (size_t)PyBytes_GET_SIZE(bytes));
    Py_DECREF(bytes);
    Py_RETURN_NONE;
}

// _devicelab_io.readline(): waits for the person's answer, with the GIL
// released so Stop (and Bonsai's screen) can still reach the interpreter.
// "" (end of input) when the run has no input.
static PyObject *devicelab_readline(PyObject *self, PyObject *unused) {
    (void)self;
    (void)unused;
    pthread_mutex_lock(&input_lock);
    int enabled = input_enabled;
    int stopped = interrupt_requested;
    if (enabled && !stopped) {
        input_waiting = 1;
        input_cancelled = 0;
    }
    pthread_mutex_unlock(&input_lock);
    if (!enabled) return PyUnicode_FromString("");
    if (stopped) {
        // Stop already scheduled a KeyboardInterrupt: raise it here instead
        // of waiting, and drop the scheduled one so it isn't raised twice.
        PyThreadState_SetAsyncExc(running_thread, NULL);
        PyErr_SetNone(PyExc_KeyboardInterrupt);
        return NULL;
    }
    emit(PYBRIDGE_INPUT_WAITING, "", 0);
    char *line = NULL;
    int cancelled = 0;
    Py_BEGIN_ALLOW_THREADS
    pthread_mutex_lock(&input_lock);
    while (input_line == NULL && !input_cancelled) {
        pthread_cond_wait(&input_ready, &input_lock);
    }
    line = input_line;
    input_line = NULL;
    cancelled = input_cancelled;
    input_cancelled = 0;
    input_waiting = 0;
    pthread_mutex_unlock(&input_lock);
    Py_END_ALLOW_THREADS
    if (cancelled) {
        free(line);
        emit(PYBRIDGE_INPUT_CANCELLED, "", 0);
        PyErr_SetNone(PyExc_KeyboardInterrupt);
        return NULL;
    }
    size_t length = strlen(line);
    emit(PYBRIDGE_INPUT_ENDED, line, length);
    // The line as input() expects it, ending in a newline.
    char *with_newline = malloc(length + 2);
    if (with_newline == NULL) {
        free(line);
        return PyErr_NoMemory();
    }
    memcpy(with_newline, line, length);
    with_newline[length] = '\n';
    with_newline[length + 1] = 0;
    free(line);
    PyObject *result = PyUnicode_DecodeUTF8(with_newline, (Py_ssize_t)length + 1, "replace");
    free(with_newline);
    return result;
}

static PyMethodDef devicelab_io_methods[] = {
    {"write", devicelab_write, METH_VARARGS, "Streams output to Python Lab's console."},
    {"readline", devicelab_readline, METH_NOARGS, "Waits for a line from Python Lab's console."},
    {NULL, NULL, 0, NULL},
};

static struct PyModuleDef devicelab_io_module = {
    PyModuleDef_HEAD_INIT, "_devicelab_io", NULL, -1, devicelab_io_methods, NULL, NULL, NULL, NULL,
};

static PyObject *devicelab_io_init(void) {
    return PyModule_Create(&devicelab_io_module);
}

static char *copy(const char *text) {
    size_t n = strlen(text);
    char *out = malloc(n + 1);
    if (out) memcpy(out, text, n + 1);
    return out;
}

static char *status_error(const char *what, PyStatus status) {
    char buffer[512];
    snprintf(buffer, sizeof buffer, "%s: %s", what,
             status.err_msg ? status.err_msg : "unknown error");
    return copy(buffer);
}

// The current Python exception as text, cleared. Needs the GIL.
static char *exception_text(const char *what) {
    PyObject *exc = PyErr_GetRaisedException();
    PyObject *text = exc ? PyObject_Str(exc) : NULL;
    const char *utf8 = text ? PyUnicode_AsUTF8(text) : NULL;
    char buffer[1024];
    snprintf(buffer, sizeof buffer, "%s: %s", what, utf8 ? utf8 : "unknown error");
    Py_XDECREF(text);
    Py_XDECREF(exc);
    PyErr_Clear();
    return copy(buffer);
}

char *pybridge_start(const char *home, const char *site_dir) {
    pthread_mutex_lock(&lock);
    if (started) {
        pthread_mutex_unlock(&lock);
        return NULL;
    }
    // iOS has no OpenSSL trust store: point ssl at the CA certificates
    // prepare-python.sh put in PYTHONHOME (OpenSSL reads this variable
    // itself, so isolated mode doesn't hide it). Without it every HTTPS
    // request fails with CERTIFICATE_VERIFY_FAILED.
    char cafile[4096];
    snprintf(cafile, sizeof cafile, "%s/etc/cacert.pem", home);
    if (access(cafile, R_OK) == 0 && getenv("SSL_CERT_FILE") == NULL) {
        setenv("SSL_CERT_FILE", cafile, 0);
    }

    // Before Py_Initialize, as the inittab requires.
    PyImport_AppendInittab("_devicelab_io", devicelab_io_init);

    PyPreConfig preconfig;
    PyPreConfig_InitIsolatedConfig(&preconfig);
    preconfig.utf8_mode = 1;
    PyStatus status = Py_PreInitialize(&preconfig);
    if (PyStatus_Exception(status)) {
        pthread_mutex_unlock(&lock);
        return status_error("Python could not pre-initialize", status);
    }

    PyConfig config;
    PyConfig_InitIsolatedConfig(&config);
    config.buffered_stdio = 0;
    // The app bundle is read-only once signed.
    config.write_bytecode = 0;
    // The app owns signals; Stop uses pybridge_interrupt instead.
    config.install_signal_handlers = 0;
    status = PyConfig_SetBytesString(&config, &config.home, home);
    if (!PyStatus_Exception(status)) status = PyConfig_Read(&config);
    if (!PyStatus_Exception(status)) status = Py_InitializeFromConfig(&config);
    PyConfig_Clear(&config);
    if (PyStatus_Exception(status)) {
        pthread_mutex_unlock(&lock);
        return status_error("Python could not start", status);
    }

    char *error = NULL;
    PyObject *main_module = PyImport_AddModule("__main__");
    PyObject *globals = main_module ? PyModule_GetDict(main_module) : NULL;
    PyObject *result = globals ? PyRun_String(RUNNER, Py_file_input, globals, globals) : NULL;
    if (result == NULL) {
        error = exception_text("Python Lab's runner did not load");
    } else {
        Py_DECREF(result);
        runner = PyDict_GetItemString(globals, "_devicelab_run");
        Py_XINCREF(runner);
        PyObject *streaming = PyRun_String(STREAM_RUNNER, Py_file_input, globals, globals);
        if (streaming == NULL) {
            error = exception_text("Python Lab's console did not load");
        } else {
            Py_DECREF(streaming);
            stream_runner = PyDict_GetItemString(globals, "_devicelab_stream");
            Py_XINCREF(stream_runner);
        }
        if (error == NULL && site_dir && *site_dir) {
            PyObject *site = PyImport_ImportModule("site");
            PyObject *added = site ? PyObject_CallMethod(site, "addsitedir", "s", site_dir) : NULL;
            if (added == NULL) error = exception_text("Python could not add its packages folder");
            Py_XDECREF(added);
            Py_XDECREF(site);
        }
    }
    started = error == NULL;
    // Let other threads take the GIL (pybridge_run, pybridge_interrupt).
    PyEval_SaveThread();
    pthread_mutex_unlock(&lock);
    return error;
}

char *pybridge_run(const char *code, int *ok) {
    *ok = 0;
    if (!started || runner == NULL) return copy("Python has not started.");
    PyGILState_STATE gil = PyGILState_Ensure();
    running_thread = PyThread_get_thread_ident();
    PyObject *result = PyObject_CallFunction(runner, "s", code);
    running_thread = 0;
    char *out;
    PyObject *item_ok, *item_text;
    if (result && PyTuple_Check(result) && PyTuple_Size(result) == 2 &&
        (item_ok = PyTuple_GetItem(result, 0)) && (item_text = PyTuple_GetItem(result, 1))) {
        *ok = PyObject_IsTrue(item_ok) == 1;
        const char *utf8 = PyUnicode_AsUTF8(item_text);
        out = copy(utf8 ? utf8 : "");
        if (!utf8) PyErr_Clear();
    } else {
        out = exception_text("Python Lab's runner failed");
    }
    Py_XDECREF(result);
    PyGILState_Release(gil);
    return out;
}

int pybridge_run_streaming(const char *code, int with_input, pybridge_write_fn write,
                           void *context) {
    if (!started || stream_runner == NULL) {
        const char *message = "Python has not started.";
        write(context, PYBRIDGE_STDERR, message, strlen(message));
        return 0;
    }
    PyGILState_STATE gil = PyGILState_Ensure();
    stream_write = write;
    stream_context = context;
    pthread_mutex_lock(&input_lock);
    input_enabled = with_input != 0;
    input_cancelled = 0;
    interrupt_requested = 0;
    free(input_line);
    input_line = NULL;
    pthread_mutex_unlock(&input_lock);
    running_thread = PyThread_get_thread_ident();
    PyObject *result =
        PyObject_CallFunction(stream_runner, "si", code, with_input != 0 ? 1 : 0);
    running_thread = 0;
    pthread_mutex_lock(&input_lock);
    input_enabled = 0;
    interrupt_requested = 0;
    pthread_mutex_unlock(&input_lock);
    int ok = 0;
    if (result != NULL) {
        ok = PyObject_IsTrue(result) == 1;
    } else {
        char *text = exception_text("Python Lab's console failed");
        if (text) emit(PYBRIDGE_STDERR, text, strlen(text));
        free(text);
    }
    Py_XDECREF(result);
    stream_write = NULL;
    stream_context = NULL;
    PyGILState_Release(gil);
    return ok;
}

int pybridge_send_input(const char *line) {
    pthread_mutex_lock(&input_lock);
    int taken = 0;
    if (input_waiting && input_line == NULL && !input_cancelled) {
        input_line = copy(line);
        taken = input_line != NULL;
        pthread_cond_signal(&input_ready);
    }
    pthread_mutex_unlock(&input_lock);
    return taken;
}

int pybridge_interrupt(void) {
    if (!started) return 0;
    PyGILState_STATE gil = PyGILState_Ensure();
    int delivered = 0;
    pthread_mutex_lock(&input_lock);
    if (input_waiting) {
        // A script waiting for input() runs no Python instruction to raise
        // KeyboardInterrupt at: wake its reader instead, which raises it.
        input_cancelled = 1;
        pthread_cond_signal(&input_ready);
        delivered = 1;
    } else if (running_thread != 0) {
        delivered = PyThreadState_SetAsyncExc(running_thread, PyExc_KeyboardInterrupt) == 1;
        // An input() the script reaches before it lands is cancelled too.
        if (delivered) interrupt_requested = 1;
    }
    pthread_mutex_unlock(&input_lock);
    PyGILState_Release(gil);
    return delivered;
}

char *pybridge_version(void) {
    return copy(Py_GetVersion());
}

void pybridge_free(char *text) {
    free(text);
}
