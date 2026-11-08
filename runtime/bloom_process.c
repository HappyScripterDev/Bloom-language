/* Bloom std::process runtime — controlled execution of external programs.
 *
 * Only emitted when the program loads `std::process`.
 *
 * Contract:
 *   - `run`/`run_in` start the program directly (no shell). Program and
 *     arguments are passed separately; on Windows the argument vector is
 *     converted to a command line with the standard MSVC quoting rules.
 *   - A start failure yields `Err(message)`; a program that ran to
 *     completion yields `Ok(ProcessOutput)` with its real exit code and the
 *     captured stdout/stderr, even when the exit code is non-zero.
 *   - stdout/stderr are captured separately and read concurrently (polling
 *     both pipes) so the child cannot deadlock on a full pipe. Output bytes
 *     are preserved exactly (Bloom strings are byte sequences).
 */

#include <errno.h>
#ifndef _WIN32
#include <unistd.h>
#include <sys/wait.h>
#include <poll.h>
#include <signal.h>
#endif

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} bloom_process_buf;

static void bloom_process_buf_init(bloom_process_buf *b) {
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
}

static void bloom_process_buf_push(bloom_process_buf *b, const char *p, size_t n) {
    if (n == 0) {
        return;
    }
    if (b->len + n + 1 > b->cap) {
        size_t cap = b->cap ? b->cap : 256;
        while (cap < b->len + n + 1) {
            cap *= 2;
        }
        char *nd = (char *)realloc(b->data, cap);
        if (nd == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        b->data = nd;
        b->cap = cap;
    }
    memcpy(b->data + b->len, p, n);
    b->len += n;
    b->data[b->len] = '\0';
}

static void bloom_process_buf_free(bloom_process_buf *b) {
    free(b->data);
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
}

static bloom_enum_Result_std_process_ProcessOutput_str bloom_process_ok(
    int64_t exit_code,
    bloom_str out,
    bloom_str err
) {
    bloom_struct_std_process_ProcessOutput o;
    o.f_exit_code = exit_code;
    o.f_stdout = out;
    o.f_stderr = err;
    bloom_enum_Result_std_process_ProcessOutput_str r;
    r.tag = 0;
    r.data.m_Ok._0 = o;
    return r;
}

static bloom_enum_Result_std_process_ProcessOutput_str bloom_process_err(bloom_str msg) {
    bloom_enum_Result_std_process_ProcessOutput_str r;
    r.tag = 1;
    r.data.m_Err._0 = msg;
    return r;
}

#ifdef _WIN32

static wchar_t *bloom_process_to_utf16(bloom_str s) {
    int need = MultiByteToWideChar(CP_UTF8, 0, s.data, (int)s.len, NULL, 0);
    if (need <= 0) {
        return NULL;
    }
    wchar_t *w = (wchar_t *)malloc(((size_t)need + 1) * sizeof(wchar_t));
    if (w == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    MultiByteToWideChar(CP_UTF8, 0, s.data, (int)s.len, w, need);
    w[need] = L'\0';
    return w;
}

/* MSVC command line quoting: an argument is quoted when it is empty or
 * contains whitespace or quotes; backslashes before a quote are doubled. */
static size_t bloom_process_quoted_len(const wchar_t *a) {
    bool need = (*a == L'\0');
    for (const wchar_t *p = a; *p != L'\0'; p++) {
        if (*p == L' ' || *p == L'\t' || *p == L'"') {
            need = true;
            break;
        }
    }
    size_t len = 0;
    size_t bs = 0;
    for (const wchar_t *p = a;; p++) {
        if (*p == L'\\') {
            bs++;
            continue;
        }
        if (*p == L'"') {
            len += need ? bs * 2 + 2 : bs + 1;
            bs = 0;
            continue;
        }
        if (*p == L'\0') {
            len += need ? bs * 2 : bs;
            break;
        }
        len += bs + 1;
        bs = 0;
    }
    return len + (need ? 2 : 0);
}

static void bloom_process_quote(wchar_t *out, const wchar_t *a) {
    bool need = (*a == L'\0');
    for (const wchar_t *p = a; *p != L'\0'; p++) {
        if (*p == L' ' || *p == L'\t' || *p == L'"') {
            need = true;
            break;
        }
    }
    wchar_t *o = out;
    if (need) {
        *o++ = L'"';
    }
    size_t bs = 0;
    for (const wchar_t *p = a;; p++) {
        if (*p == L'\\') {
            bs++;
            continue;
        }
        if (*p == L'"') {
            size_t n = need ? bs * 2 + 1 : bs + 1;
            for (size_t i = 0; i < n; i++) {
                *o++ = L'\\';
            }
            *o++ = L'"';
            bs = 0;
            continue;
        }
        if (*p == L'\0') {
            size_t n = need ? bs * 2 : bs;
            for (size_t i = 0; i < n; i++) {
                *o++ = L'\\';
            }
            break;
        }
        while (bs > 0) {
            *o++ = L'\\';
            bs--;
        }
        *o++ = *p;
    }
    if (need) {
        *o++ = L'"';
    }
    *o = L'\0';
}

static wchar_t *bloom_process_build_cmdline(const wchar_t *program, bloom_arr_str args) {
    size_t total = bloom_process_quoted_len(program) + 1;
    for (size_t i = 0; i < args->len; i++) {
        wchar_t *a = bloom_process_to_utf16(args->data[i]);
        if (a == NULL) {
            return NULL;
        }
        total += bloom_process_quoted_len(a) + 1;
        free(a);
    }
    wchar_t *line = (wchar_t *)malloc(total * sizeof(wchar_t));
    if (line == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    bloom_process_quote(line, program);
    size_t at = wcslen(line);
    for (size_t i = 0; i < args->len; i++) {
        wchar_t *a = bloom_process_to_utf16(args->data[i]);
        if (a == NULL) {
            free(line);
            return NULL;
        }
        line[at++] = L' ';
        bloom_process_quote(line + at, a);
        at += wcslen(line + at);
        free(a);
    }
    line[at] = L'\0';
    return line;
}

static const char *bloom_process_win_reason(DWORD e) {
    switch (e) {
        case ERROR_FILE_NOT_FOUND:
            return "program not found";
        case ERROR_PATH_NOT_FOUND:
            return "path not found";
        case ERROR_ACCESS_DENIED:
            return "access denied";
        case ERROR_BAD_EXE_FORMAT:
            return "not a valid executable";
        case ERROR_NOT_ENOUGH_MEMORY:
            return "not enough memory";
        case ERROR_DIRECTORY:
            return "working directory is not a directory";
        default:
            return NULL;
    }
}

static bloom_str bloom_process_win_err(const char *op, bloom_str what, DWORD e) {
    const char *reason = bloom_process_win_reason(e);
    char num[48];
    if (reason == NULL) {
        snprintf(num, sizeof num, "system error %lu", (unsigned long)e);
        reason = num;
    }
    size_t opn = strlen(op);
    size_t rn = strlen(reason);
    size_t total = opn + 2 + what.len + 2 + rn;
    char *buf = (char *)malloc(total + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t at = 0;
    memcpy(buf + at, op, opn);
    at += opn;
    buf[at++] = ':';
    buf[at++] = ' ';
    if (what.len > 0) {
        memcpy(buf + at, what.data, what.len);
        at += what.len;
    }
    buf[at++] = ':';
    buf[at++] = ' ';
    memcpy(buf + at, reason, rn);
    at += rn;
    bloom_str msg = bloom_str_from_bytes(buf, at);
    free(buf);
    return msg;
}

static bool bloom_process_drain(bloom_process_buf *b, HANDLE h) {
    DWORD avail = 0;
    if (!PeekNamedPipe(h, NULL, 0, NULL, &avail, NULL)) {
        return true;
    }
    while (avail > 0) {
        char tmp[4096];
        DWORD want = avail > (DWORD)sizeof tmp ? (DWORD)sizeof tmp : avail;
        DWORD got = 0;
        if (!ReadFile(h, tmp, want, &got, NULL) || got == 0) {
            return true;
        }
        bloom_process_buf_push(b, tmp, (size_t)got);
        if (!PeekNamedPipe(h, NULL, 0, NULL, &avail, NULL)) {
            return true;
        }
    }
    return false;
}

#endif /* _WIN32 */

bloom_enum_Result_std_process_ProcessOutput_str bloom_process_run_in(
    bloom_str program,
    bloom_arr_str args,
    bloom_str cwd
) {
#ifdef _WIN32
    wchar_t *wprog = bloom_process_to_utf16(program);
    if (wprog == NULL) {
        return bloom_process_err(bloom_process_win_err(
            "run",
            program,
            ERROR_INVALID_NAME
        ));
    }
    wchar_t *wline = bloom_process_build_cmdline(wprog, args);
    if (wline == NULL) {
        free(wprog);
        return bloom_process_err(bloom_process_win_err(
            "run",
            program,
            ERROR_INVALID_NAME
        ));
    }
    wchar_t *wcwd = NULL;
    if (cwd.len > 0) {
        wcwd = bloom_process_to_utf16(cwd);
        if (wcwd == NULL) {
            free(wprog);
            free(wline);
            return bloom_process_err(bloom_process_win_err(
                "run_in",
                cwd,
                ERROR_INVALID_NAME
            ));
        }
    }
    SECURITY_ATTRIBUTES sa;
    sa.nLength = sizeof sa;
    sa.lpSecurityDescriptor = NULL;
    sa.bInheritHandle = TRUE;
    HANDLE out_read = NULL;
    HANDLE out_write = NULL;
    HANDLE err_read = NULL;
    HANDLE err_write = NULL;
    if (!CreatePipe(&out_read, &out_write, &sa, 0)
        || !CreatePipe(&err_read, &err_write, &sa, 0)) {
        if (out_read) CloseHandle(out_read);
        if (out_write) CloseHandle(out_write);
        if (err_read) CloseHandle(err_read);
        if (err_write) CloseHandle(err_write);
        free(wprog);
        free(wline);
        free(wcwd);
        return bloom_process_err(bloom_process_win_err("run", program, GetLastError()));
    }
    SetHandleInformation(out_read, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(err_read, HANDLE_FLAG_INHERIT, 0);
    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    memset(&si, 0, sizeof si);
    memset(&pi, 0, sizeof pi);
    si.cb = sizeof si;
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    si.hStdOutput = out_write;
    si.hStdError = err_write;
    /* lpApplicationName = NULL: the (quoted) program name in the command
     * line selects the executable, and the system searches PATH. */
    BOOL started = CreateProcessW(
        NULL,
        wline,
        NULL,
        NULL,
        TRUE,
        0,
        NULL,
        wcwd,
        &si,
        &pi
    );
    DWORD start_error = started ? 0 : GetLastError();
    CloseHandle(out_write);
    CloseHandle(err_write);
    free(wprog);
    free(wline);
    free(wcwd);
    if (!started) {
        CloseHandle(out_read);
        CloseHandle(err_read);
        return bloom_process_err(bloom_process_win_err("run", program, start_error));
    }
    bloom_process_buf out;
    bloom_process_buf err;
    bloom_process_buf_init(&out);
    bloom_process_buf_init(&err);
    bool exited = false;
    bool out_done = false;
    bool err_done = false;
    for (;;) {
        if (!out_done) {
            out_done = bloom_process_drain(&out, out_read);
        }
        if (!err_done) {
            err_done = bloom_process_drain(&err, err_read);
        }
        if (!exited) {
            exited = WaitForSingleObject(pi.hProcess, 5) == WAIT_OBJECT_0;
        }
        if (exited && out_done && err_done) {
            break;
        }
        if (out_done && err_done) {
            Sleep(1);
        }
    }
    DWORD exit_code = 0;
    GetExitCodeProcess(pi.hProcess, &exit_code);
    CloseHandle(pi.hProcess);
    CloseHandle(pi.hThread);
    CloseHandle(out_read);
    CloseHandle(err_read);
    bloom_str out_str = bloom_str_from_bytes(out.data ? out.data : "", out.len);
    bloom_str err_str = bloom_str_from_bytes(err.data ? err.data : "", err.len);
    bloom_process_buf_free(&out);
    bloom_process_buf_free(&err);
    return bloom_process_ok((int64_t)exit_code, out_str, err_str);
#else
    int outpipe[2];
    int errpipe[2];
    if (pipe(outpipe) != 0 || pipe(errpipe) != 0) {
        return bloom_process_err(bloom_process_win_err("run", program, (DWORD)errno));
    }
    char **argv = (char **)malloc((args->len + 2) * sizeof(char *));
    if (argv == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    argv[0] = (char *)malloc(program.len + 1);
    memcpy(argv[0], program.data, program.len);
    argv[0][program.len] = '\0';
    for (size_t i = 0; i < args->len; i++) {
        argv[i + 1] = (char *)malloc(args->data[i].len + 1);
        memcpy(argv[i + 1], args->data[i].data, args->data[i].len);
        argv[i + 1][args->data[i].len] = '\0';
    }
    argv[args->len + 1] = NULL;
    pid_t pid = fork();
    if (pid < 0) {
        for (size_t i = 0; i <= args->len; i++) {
            free(argv[i]);
        }
        free(argv);
        close(outpipe[0]);
        close(outpipe[1]);
        close(errpipe[0]);
        close(errpipe[1]);
        return bloom_process_err(bloom_process_win_err("run", program, (DWORD)errno));
    }
    if (pid == 0) {
        dup2(outpipe[1], STDOUT_FILENO);
        dup2(errpipe[1], STDERR_FILENO);
        close(outpipe[0]);
        close(outpipe[1]);
        close(errpipe[0]);
        close(errpipe[1]);
        if (cwd.len > 0) {
            char *dir = (char *)malloc(cwd.len + 1);
            memcpy(dir, cwd.data, cwd.len);
            dir[cwd.len] = '\0';
            if (chdir(dir) != 0) {
                _exit(127);
            }
            free(dir);
        }
        execvp(argv[0], argv);
        _exit(127);
    }
    for (size_t i = 0; i <= args->len; i++) {
        free(argv[i]);
    }
    free(argv);
    close(outpipe[1]);
    close(errpipe[1]);
    bloom_process_buf out;
    bloom_process_buf err;
    bloom_process_buf_init(&out);
    bloom_process_buf_init(&err);
    bool out_done = false;
    bool err_done = false;
    while (!out_done || !err_done) {
        struct pollfd fds[2];
        int n = 0;
        if (!out_done) {
            fds[n].fd = outpipe[0];
            fds[n].events = POLLIN;
            n++;
        }
        if (!err_done) {
            fds[n].fd = errpipe[0];
            fds[n].events = POLLIN;
            n++;
        }
        if (poll(fds, (nfds_t)n, -1) < 0) {
            break;
        }
        for (int i = 0; i < n; i++) {
            if (fds[i].revents & (POLLIN | POLLHUP | POLLERR)) {
                char tmp[4096];
                ssize_t got = read(fds[i].fd, tmp, sizeof tmp);
                if (got > 0) {
                    if (fds[i].fd == outpipe[0]) {
                        bloom_process_buf_push(&out, tmp, (size_t)got);
                    } else {
                        bloom_process_buf_push(&err, tmp, (size_t)got);
                    }
                } else {
                    if (fds[i].fd == outpipe[0]) {
                        out_done = true;
                        close(outpipe[0]);
                    } else {
                        err_done = true;
                        close(errpipe[0]);
                    }
                }
            }
        }
    }
    int status = 0;
    waitpid(pid, &status, 0);
    int64_t exit_code = 0;
    if (WIFEXITED(status)) {
        exit_code = (int64_t)WEXITSTATUS(status);
    } else if (WIFSIGNALED(status)) {
        exit_code = 128 + (int64_t)WTERMSIG(status);
    }
    bloom_str out_str = bloom_str_from_bytes(out.data ? out.data : "", out.len);
    bloom_str err_str = bloom_str_from_bytes(err.data ? err.data : "", err.len);
    bloom_process_buf_free(&out);
    bloom_process_buf_free(&err);
    return bloom_process_ok(exit_code, out_str, err_str);
#endif
}

bloom_enum_Result_std_process_ProcessOutput_str bloom_process_run(
    bloom_str program,
    bloom_arr_str args
) {
    bloom_str empty;
    empty.rc = 0;
    empty.data = NULL;
    empty.len = 0;
    return bloom_process_run_in(program, args, empty);
}

/* ---- 0.23: spawn, wait, timeout and output limits --------------------- */

#ifdef _WIN32

typedef struct {
    HANDLE pipe;
    bloom_process_buf buf;
    size_t limit;
    bool overflow;
} bloom_process_reader;

typedef struct {
    int rc;
    HANDLE process;
    bloom_process_reader out_reader;
    bloom_process_reader err_reader;
    HANDLE out_thread;
    HANDLE err_thread;
    int64_t timeout_ms;
    bool finished;
    DWORD exit_code;
} bloom_process_child;

static DWORD WINAPI bloom_process_reader_main(LPVOID param) {
    bloom_process_reader *r = (bloom_process_reader *)param;
    char tmp[4096];
    DWORD got = 0;
    while (ReadFile(r->pipe, tmp, sizeof tmp, &got, NULL) && got > 0) {
        size_t room = r->buf.len < r->limit ? r->limit - r->buf.len : 0;
        if (got <= room) {
            bloom_process_buf_push(&r->buf, tmp, (size_t)got);
        } else {
            if (room > 0) {
                bloom_process_buf_push(&r->buf, tmp, room);
            }
            r->overflow = true;
        }
    }
    CloseHandle(r->pipe);
    r->pipe = NULL;
    return 0;
}

static bloom_process_child *bloom_process_child_from(bloom_process_handle h) {
    return (bloom_process_child *)(intptr_t)h;
}

bloom_process_handle bloom_process_child_retain(bloom_process_handle h) {
    bloom_process_child *child = bloom_process_child_from(h);
    if (child != NULL) {
        child->rc++;
    }
    return h;
}

static void bloom_process_child_finalize(bloom_process_child *child) {
    if (child == NULL) {
        return;
    }
    if (child->process != NULL) {
        DWORD code = 0;
        if (WaitForSingleObject(child->process, 0) == WAIT_OBJECT_0) {
            GetExitCodeProcess(child->process, &code);
            child->exit_code = code;
            child->finished = true;
        }
    }
    if (child->out_thread != NULL) {
        WaitForSingleObject(child->out_thread, INFINITE);
        CloseHandle(child->out_thread);
        child->out_thread = NULL;
    }
    if (child->err_thread != NULL) {
        WaitForSingleObject(child->err_thread, INFINITE);
        CloseHandle(child->err_thread);
        child->err_thread = NULL;
    }
    if (child->process != NULL) {
        DWORD code = 0;
        GetExitCodeProcess(child->process, &code);
        child->exit_code = code;
        CloseHandle(child->process);
        child->process = NULL;
        child->finished = true;
    }
}

void bloom_process_child_release(bloom_process_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_process_child *child = bloom_process_child_from(*h);
    *h = 0;
    if (child == NULL) {
        return;
    }
    if (child->rc > 0) {
        child->rc--;
    }
    if (child->rc != 0) {
        return;
    }
    if (!child->finished && child->process != NULL
        && WaitForSingleObject(child->process, 0) == WAIT_TIMEOUT) {
        TerminateProcess(child->process, 1);
    }
    bloom_process_child_finalize(child);
    bloom_process_buf_free(&child->out_reader.buf);
    bloom_process_buf_free(&child->err_reader.buf);
    free(child);
}

static bloom_struct_std_process_Process bloom_process_wrap_child(
    bloom_process_child *child
) {
    bloom_struct_std_process_Process v;
    v.f_id = (bloom_process_handle)(intptr_t)child;
    return v;
}

bloom_enum_Result_std_process_Process_str bloom_process_spawn(
    bloom_str program,
    bloom_arr_str args,
    bloom_struct_std_process_ProcessOptions options
) {
    bloom_enum_Result_std_process_Process_str r;
    size_t limit = 8 * 1024 * 1024;
    if (options.f_max_output > 0) {
        limit = (size_t)options.f_max_output;
    }
    if (limit > 64 * 1024 * 1024) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_str_from_bytes(
            "spawn: max_output exceeds 67108864 bytes",
            43
        );
        return r;
    }
    if (options.f_timeout_ms < 0 || options.f_timeout_ms > 86400000) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_str_from_bytes(
            "spawn: timeout_ms must be between 0 and 86400000",
            50
        );
        return r;
    }
    wchar_t *wprog = bloom_process_to_utf16(program);
    if (wprog == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_process_win_err("spawn", program, ERROR_INVALID_NAME);
        return r;
    }
    wchar_t *wline = bloom_process_build_cmdline(wprog, args);
    if (wline == NULL) {
        free(wprog);
        r.tag = 1;
        r.data.m_Err._0 = bloom_process_win_err("spawn", program, ERROR_INVALID_NAME);
        return r;
    }
    wchar_t *wcwd = NULL;
    if (options.f_directory.len > 0) {
        wcwd = bloom_process_to_utf16(options.f_directory);
        if (wcwd == NULL) {
            free(wprog);
            free(wline);
            r.tag = 1;
            r.data.m_Err._0 = bloom_process_win_err(
                "spawn",
                options.f_directory,
                ERROR_INVALID_NAME
            );
            return r;
        }
    }
    SECURITY_ATTRIBUTES sa;
    sa.nLength = sizeof sa;
    sa.lpSecurityDescriptor = NULL;
    sa.bInheritHandle = TRUE;
    HANDLE out_read = NULL;
    HANDLE out_write = NULL;
    HANDLE err_read = NULL;
    HANDLE err_write = NULL;
    if (!CreatePipe(&out_read, &out_write, &sa, 0)
        || !CreatePipe(&err_read, &err_write, &sa, 0)) {
        if (out_read) CloseHandle(out_read);
        if (out_write) CloseHandle(out_write);
        if (err_read) CloseHandle(err_read);
        if (err_write) CloseHandle(err_write);
        free(wprog);
        free(wline);
        free(wcwd);
        r.tag = 1;
        r.data.m_Err._0 = bloom_process_win_err("spawn", program, GetLastError());
        return r;
    }
    SetHandleInformation(out_read, HANDLE_FLAG_INHERIT, 0);
    SetHandleInformation(err_read, HANDLE_FLAG_INHERIT, 0);
    STARTUPINFOW si;
    PROCESS_INFORMATION pi;
    memset(&si, 0, sizeof si);
    memset(&pi, 0, sizeof pi);
    si.cb = sizeof si;
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    si.hStdOutput = out_write;
    si.hStdError = err_write;
    BOOL started = CreateProcessW(
        NULL,
        wline,
        NULL,
        NULL,
        TRUE,
        0,
        NULL,
        wcwd,
        &si,
        &pi
    );
    DWORD start_error = started ? 0 : GetLastError();
    CloseHandle(out_write);
    CloseHandle(err_write);
    free(wprog);
    free(wline);
    free(wcwd);
    if (!started) {
        CloseHandle(out_read);
        CloseHandle(err_read);
        r.tag = 1;
        r.data.m_Err._0 = bloom_process_win_err("spawn", program, start_error);
        return r;
    }
    bloom_process_child *child = (bloom_process_child *)calloc(1, sizeof *child);
    if (child == NULL) {
        TerminateProcess(pi.hProcess, 1);
        WaitForSingleObject(pi.hProcess, INFINITE);
        CloseHandle(pi.hProcess);
        CloseHandle(pi.hThread);
        CloseHandle(out_read);
        CloseHandle(err_read);
        bloom_panicf(NULL, 0, "out of memory");
    }
    child->rc = 1;
    child->process = pi.hProcess;
    child->timeout_ms = options.f_timeout_ms;
    child->out_reader.pipe = out_read;
    child->out_reader.limit = limit;
    child->err_reader.pipe = err_read;
    child->err_reader.limit = limit;
    child->out_thread = CreateThread(
        NULL,
        0,
        bloom_process_reader_main,
        &child->out_reader,
        0,
        NULL
    );
    child->err_thread = CreateThread(
        NULL,
        0,
        bloom_process_reader_main,
        &child->err_reader,
        0,
        NULL
    );
    CloseHandle(pi.hThread);
    if (child->out_thread == NULL || child->err_thread == NULL) {
        TerminateProcess(child->process, 1);
        bloom_process_child_finalize(child);
        bloom_process_buf_free(&child->out_reader.buf);
        bloom_process_buf_free(&child->err_reader.buf);
        free(child);
        r.tag = 1;
        r.data.m_Err._0 = bloom_process_win_err("spawn", program, GetLastError());
        return r;
    }
    r.tag = 0;
    r.data.m_Ok._0 = bloom_process_wrap_child(child);
    return r;
}

bloom_enum_Result_std_process_ProcessOutput_str bloom_process_wait(
    bloom_struct_std_process_Process handle
) {
    bloom_enum_Result_std_process_ProcessOutput_str r;
    bloom_process_child *child = bloom_process_child_from(handle.f_id);
    if (child == NULL || child->process == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_str_from_bytes("wait: invalid process handle", 29);
        return r;
    }
    DWORD timeout = child->timeout_ms > 0 ? (DWORD)child->timeout_ms : INFINITE;
    DWORD waited = WaitForSingleObject(child->process, timeout);
    if (waited == WAIT_TIMEOUT) {
        TerminateProcess(child->process, 1);
        WaitForSingleObject(child->process, INFINITE);
        bloom_process_child_finalize(child);
        bloom_process_buf_free(&child->out_reader.buf);
        bloom_process_buf_free(&child->err_reader.buf);
        child->out_reader.buf.data = NULL;
        child->out_reader.buf.len = 0;
        child->err_reader.buf.data = NULL;
        child->err_reader.buf.len = 0;
        char msg[96];
        snprintf(msg, sizeof msg, "wait: process timed out after %lld ms", (long long)child->timeout_ms);
        r.tag = 1;
        r.data.m_Err._0 = bloom_str_from_bytes(msg, strlen(msg));
        return r;
    }
    if (waited != WAIT_OBJECT_0) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_process_win_err("wait", bloom_str_from_bytes("process", 7), GetLastError());
        return r;
    }
    DWORD code = 0;
    GetExitCodeProcess(child->process, &code);
    child->exit_code = code;
    bloom_process_child_finalize(child);
    if (child->out_reader.overflow || child->err_reader.overflow) {
        bloom_process_buf_free(&child->out_reader.buf);
        bloom_process_buf_free(&child->err_reader.buf);
        child->out_reader.buf.data = NULL;
        child->out_reader.buf.len = 0;
        child->err_reader.buf.data = NULL;
        child->err_reader.buf.len = 0;
        r.tag = 1;
        r.data.m_Err._0 = bloom_str_from_bytes(
            "wait: process output exceeded the configured limit",
            51
        );
        return r;
    }
    bloom_str out = bloom_str_from_bytes(
        child->out_reader.buf.data ? child->out_reader.buf.data : "",
        child->out_reader.buf.len
    );
    bloom_str err = bloom_str_from_bytes(
        child->err_reader.buf.data ? child->err_reader.buf.data : "",
        child->err_reader.buf.len
    );
    bloom_process_buf_free(&child->out_reader.buf);
    bloom_process_buf_free(&child->err_reader.buf);
    child->out_reader.buf.data = NULL;
    child->out_reader.buf.len = 0;
    child->err_reader.buf.data = NULL;
    child->err_reader.buf.len = 0;
    r.tag = 0;
    r.data.m_Ok._0.f_exit_code = (int64_t)code;
    r.data.m_Ok._0.f_stdout = out;
    r.data.m_Ok._0.f_stderr = err;
    return r;
}

bloom_enum_Option_i64 bloom_process_try_wait(
    bloom_struct_std_process_Process handle
) {
    bloom_enum_Option_i64 r;
    bloom_process_child *child = bloom_process_child_from(handle.f_id);
    if (child == NULL || child->process == NULL) {
        r.tag = 1;
        return r;
    }
    DWORD code = 0;
    if (WaitForSingleObject(child->process, 0) == WAIT_OBJECT_0) {
        GetExitCodeProcess(child->process, &code);
        child->exit_code = code;
        child->finished = true;
        r.tag = 0;
        r.data.m_Some._0 = (int64_t)code;
        return r;
    }
    r.tag = 1;
    return r;
}

bloom_enum_Result_bool_str bloom_process_kill(
    bloom_struct_std_process_Process handle
) {
    bloom_enum_Result_bool_str r;
    bloom_process_child *child = bloom_process_child_from(handle.f_id);
    if (child == NULL || child->process == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_str_from_bytes("kill: invalid process handle", 29);
        return r;
    }
    if (WaitForSingleObject(child->process, 0) == WAIT_OBJECT_0) {
        r.tag = 0;
        r.data.m_Ok._0 = false;
        return r;
    }
    BOOL ok = TerminateProcess(child->process, 1);
    if (!ok) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_process_win_err("kill", bloom_str_from_bytes("process", 7), GetLastError());
        return r;
    }
    WaitForSingleObject(child->process, INFINITE);
    r.tag = 0;
    r.data.m_Ok._0 = true;
    return r;
}

bloom_enum_Result_std_process_ProcessOutput_str bloom_process_run_options(
    bloom_str program,
    bloom_arr_str args,
    bloom_struct_std_process_ProcessOptions options
) {
    bloom_enum_Result_std_process_ProcessOutput_str r;
    bloom_enum_Result_std_process_Process_str spawned = bloom_process_spawn(
        program,
        args,
        options
    );
    if (spawned.tag == 1) {
        r.tag = 1;
        r.data.m_Err._0 = spawned.data.m_Err._0;
        return r;
    }
    r = bloom_process_wait(spawned.data.m_Ok._0);
    bloom_process_child_release(&spawned.data.m_Ok._0.f_id);
    return r;
}

int64_t bloom_process_output_exit_code(
    bloom_struct_std_process_ProcessOutput output
) {
    return output.f_exit_code;
}

bool bloom_process_output_success(
    bloom_struct_std_process_ProcessOutput output
) {
    return output.f_exit_code == 0;
}

bloom_str bloom_process_output_stdout(
    bloom_struct_std_process_ProcessOutput output
) {
    return bloom_str_clone(output.f_stdout);
}

bloom_str bloom_process_output_stderr(
    bloom_struct_std_process_ProcessOutput output
) {
    return bloom_str_clone(output.f_stderr);
}

#else /* !_WIN32 */

bloom_process_handle bloom_process_child_retain(bloom_process_handle h) {
    return h;
}

void bloom_process_child_release(bloom_process_handle *h) {
    if (h != NULL) {
        *h = 0;
    }
}

bloom_enum_Result_std_process_Process_str bloom_process_spawn(
    bloom_str program,
    bloom_arr_str args,
    bloom_struct_std_process_ProcessOptions options
) {
    bloom_enum_Result_std_process_Process_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_str_from_bytes(
        "spawn: only supported on Windows",
        32
    );
    return r;
}

bloom_enum_Result_std_process_ProcessOutput_str bloom_process_wait(
    bloom_struct_std_process_Process handle
) {
    bloom_enum_Result_std_process_ProcessOutput_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_str_from_bytes(
        "wait: only supported on Windows",
        30
    );
    return r;
}

bloom_enum_Option_i64 bloom_process_try_wait(
    bloom_struct_std_process_Process handle
) {
    bloom_enum_Option_i64 r;
    r.tag = 1;
    return r;
}

bloom_enum_Result_bool_str bloom_process_kill(
    bloom_struct_std_process_Process handle
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_str_from_bytes(
        "kill: only supported on Windows",
        30
    );
    return r;
}

bloom_enum_Result_std_process_ProcessOutput_str bloom_process_run_options(
    bloom_str program,
    bloom_arr_str args,
    bloom_struct_std_process_ProcessOptions options
) {
    bloom_str empty = {0, NULL, 0};
    (void)options;
    return bloom_process_run_in(program, args, empty);
}

int64_t bloom_process_output_exit_code(
    bloom_struct_std_process_ProcessOutput output
) {
    return output.f_exit_code;
}

bool bloom_process_output_success(
    bloom_struct_std_process_ProcessOutput output
) {
    return output.f_exit_code == 0;
}

bloom_str bloom_process_output_stdout(
    bloom_struct_std_process_ProcessOutput output
) {
    return bloom_str_clone(output.f_stdout);
}

bloom_str bloom_process_output_stderr(
    bloom_struct_std_process_ProcessOutput output
) {
    return bloom_str_clone(output.f_stderr);
}

#endif