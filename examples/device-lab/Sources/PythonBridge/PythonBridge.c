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
// The runner function defined by RUNNER below.
static PyObject *runner = NULL;
// The thread running a script, for pybridge_interrupt (0 when none).
static unsigned long running_thread = 0;

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
        if (site_dir && *site_dir) {
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

int pybridge_interrupt(void) {
    if (!started) return 0;
    PyGILState_STATE gil = PyGILState_Ensure();
    int delivered = 0;
    if (running_thread != 0) {
        delivered = PyThreadState_SetAsyncExc(running_thread, PyExc_KeyboardInterrupt) == 1;
    }
    PyGILState_Release(gil);
    return delivered;
}

char *pybridge_version(void) {
    return copy(Py_GetVersion());
}

void pybridge_free(char *text) {
    free(text);
}
