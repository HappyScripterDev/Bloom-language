/* Bloom std::env runtime - arguments, environment, working directory.
 *
 * Windows uses the Unicode APIs (UTF-8 at the Bloom boundary). Missing
 * environment variables are distinct from empty values. Secrets are never
 * printed by the runtime.
 */

#ifdef _WIN32

#include <shellapi.h>

#define BLOOM_ENV_MAX_ARGS 4096
#define BLOOM_ENV_MAX_ARG_BYTES (1024 * 1024)
#define BLOOM_ENV_MAX_VALUE 32767

static wchar_t *bloom_env_to_utf16(bloom_str s) {
    if (s.len == 0) {
        wchar_t *empty = (wchar_t *)malloc(sizeof(wchar_t));
        if (empty == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        empty[0] = L'\0';
        return empty;
    }
    int need = MultiByteToWideChar(
        CP_UTF8,
        MB_ERR_INVALID_CHARS,
        s.data,
        (int)s.len,
        NULL,
        0
    );
    if (need <= 0) {
        return NULL;
    }
    wchar_t *w = (wchar_t *)malloc(((size_t)need + 1) * sizeof(wchar_t));
    if (w == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, s.data, (int)s.len, w, need);
    w[need] = L'\0';
    return w;
}

static bloom_str bloom_env_from_utf16(const wchar_t *w, int wlen) {
    if (wlen <= 0) {
        return bloom_str_from_bytes("", 0);
    }
    int need = WideCharToMultiByte(CP_UTF8, 0, w, wlen, NULL, 0, NULL, NULL);
    if (need <= 0) {
        return bloom_str_from_bytes("", 0);
    }
    char *buf = (char *)malloc((size_t)need + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    WideCharToMultiByte(CP_UTF8, 0, w, wlen, buf, need, NULL, NULL);
    bloom_str s = bloom_str_from_bytes(buf, (size_t)need);
    free(buf);
    return s;
}

/* Windows cannot store empty environment values (setting "" removes the
 * variable). Bloom keeps them in a small process-local table so that
 * missing and empty remain distinguishable inside the process. */
#define BLOOM_ENV_EMPTY_MAX 64
#define BLOOM_ENV_EMPTY_NAME 511
static char bloom_env_empty_names[BLOOM_ENV_EMPTY_MAX][BLOOM_ENV_EMPTY_NAME + 1];
static int bloom_env_empty_count = 0;

static int bloom_env_empty_index(bloom_str name) {
    if (name.len == 0 || name.len > BLOOM_ENV_EMPTY_NAME) {
        return -1;
    }
    for (int i = 0; i < bloom_env_empty_count; i++) {
        if (strlen(bloom_env_empty_names[i]) == name.len
            && memcmp(bloom_env_empty_names[i], name.data, name.len) == 0) {
            return i;
        }
    }
    return -1;
}

static void bloom_env_empty_remove(bloom_str name) {
    int index = bloom_env_empty_index(name);
    if (index < 0) {
        return;
    }
    for (int i = index; i + 1 < bloom_env_empty_count; i++) {
        memcpy(
            bloom_env_empty_names[i],
            bloom_env_empty_names[i + 1],
            BLOOM_ENV_EMPTY_NAME + 1
        );
    }
    bloom_env_empty_count--;
}

static bool bloom_env_empty_add(bloom_str name) {
    if (bloom_env_empty_index(name) >= 0) {
        return true;
    }
    if (name.len > BLOOM_ENV_EMPTY_NAME
        || bloom_env_empty_count >= BLOOM_ENV_EMPTY_MAX) {
        return false;
    }
    memcpy(
        bloom_env_empty_names[bloom_env_empty_count],
        name.data,
        name.len
    );
    bloom_env_empty_names[bloom_env_empty_count][name.len] = '\0';
    bloom_env_empty_count++;
    return true;
}

static bloom_str bloom_env_msg(const char *op, const char *detail) {
    size_t opn = strlen(op);
    size_t dn = strlen(detail);
    char *buf = (char *)malloc(opn + 2 + dn + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(buf, op, opn);
    buf[opn] = ':';
    buf[opn + 1] = ' ';
    memcpy(buf + opn + 2, detail, dn + 1);
    bloom_str s = bloom_str_from_bytes(buf, opn + 2 + dn);
    free(buf);
    return s;
}

static bloom_str bloom_env_werr(const char *op, DWORD code) {
    char buf[96];
    snprintf(buf, sizeof buf, "%s: Windows error %lu", op, (unsigned long)code);
    return bloom_str_from_bytes(buf, strlen(buf));
}

bloom_arr_str bloom_env_args(void) {
    int count = 0;
    LPWSTR *argv = CommandLineToArgvW(GetCommandLineW(), &count);
    bloom_arr_str out = bloom_arr_str_new();
    if (argv == NULL) {
        return out;
    }
    int limit = count;
    if (limit > BLOOM_ENV_MAX_ARGS) {
        limit = BLOOM_ENV_MAX_ARGS;
    }
    for (int i = 0; i < limit; i++) {
        int len = (int)wcslen(argv[i]);
        bloom_str s = bloom_env_from_utf16(argv[i], len);
        if (s.len > BLOOM_ENV_MAX_ARG_BYTES) {
            bloom_str_drop(&s);
            continue;
        }
        bloom_arr_str_push_owned(out, s);
    }
    LocalFree(argv);
    return out;
}

static bool bloom_env_lookup(bloom_str name, bloom_str *value, bool *found) {
    *found = false;
    value->data = NULL;
    value->len = 0;
    value->rc = 0;
    if (name.len == 0 || name.len > BLOOM_ENV_MAX_VALUE) {
        return false;
    }
    if (bloom_env_empty_index(name) >= 0) {
        *found = true;
        *value = bloom_str_from_bytes("", 0);
        return true;
    }
    wchar_t *wname = bloom_env_to_utf16(name);
    if (wname == NULL) {
        return false;
    }
    SetLastError(ERROR_SUCCESS);
    DWORD need = GetEnvironmentVariableW(wname, NULL, 0);
    if (need == 0) {
        DWORD err = GetLastError();
        free(wname);
        if (err == ERROR_ENVVAR_NOT_FOUND) {
            return true;
        }
        *found = true;
        *value = bloom_str_from_bytes("", 0);
        return true;
    }
    wchar_t *buf = (wchar_t *)malloc((size_t)need * sizeof(wchar_t));
    if (buf == NULL) {
        free(wname);
        bloom_panicf(NULL, 0, "out of memory");
    }
    DWORD got = GetEnvironmentVariableW(wname, buf, need);
    free(wname);
    if (got == 0) {
        DWORD err = GetLastError();
        free(buf);
        if (err == ERROR_ENVVAR_NOT_FOUND) {
            return true;
        }
        return true;
    }
    *value = bloom_env_from_utf16(buf, (int)got);
    *found = true;
    free(buf);
    return true;
}

bloom_enum_Option_str bloom_env_get(bloom_str name) {
    bloom_enum_Option_str r;
    bloom_str value;
    bool found = false;
    bloom_env_lookup(name, &value, &found);
    if (!found) {
        bloom_str_drop(&value);
        r.tag = 1;
        return r;
    }
    r.tag = 0;
    r.data.m_Some._0 = value;
    return r;
}

bool bloom_env_has(bloom_str name) {
    bloom_str value;
    bool found = false;
    bloom_env_lookup(name, &value, &found);
    bloom_str_drop(&value);
    return found;
}

bloom_enum_Result_bool_str bloom_env_set(bloom_str name, bloom_str value) {
    bloom_enum_Result_bool_str r;
    if (name.len == 0 || name.len > BLOOM_ENV_MAX_VALUE
        || memchr(name.data, '=', name.len) != NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_msg(
            "set",
            "variable name must be 1..32767 bytes and must not contain `=`"
        );
        return r;
    }
    if (value.len > BLOOM_ENV_MAX_VALUE) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_msg("set", "value exceeds 32767 bytes");
        return r;
    }
    if (value.len == 0) {
        wchar_t *wname = bloom_env_to_utf16(name);
        if (wname == NULL) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_env_msg("set", "name is not valid UTF-8");
            return r;
        }
        BOOL ok = SetEnvironmentVariableW(wname, NULL);
        DWORD err = ok ? 0 : GetLastError();
        free(wname);
        if (!ok) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_env_werr("set", err);
            return r;
        }
        if (!bloom_env_empty_add(name)) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_env_msg(
                "set",
                "empty-value table is full or name exceeds 511 bytes"
            );
            return r;
        }
        r.tag = 0;
        r.data.m_Ok._0 = true;
        return r;
    }
    bloom_env_empty_remove(name);
    wchar_t *wname = bloom_env_to_utf16(name);
    wchar_t *wvalue = bloom_env_to_utf16(value);
    if (wname == NULL || wvalue == NULL) {
        free(wname);
        free(wvalue);
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_msg("set", "name or value is not valid UTF-8");
        return r;
    }
    BOOL ok = SetEnvironmentVariableW(wname, wvalue);
    DWORD err = ok ? 0 : GetLastError();
    free(wname);
    free(wvalue);
    if (!ok) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_werr("set", err);
        return r;
    }
    r.tag = 0;
    r.data.m_Ok._0 = true;
    return r;
}

bloom_enum_Result_bool_str bloom_env_remove(bloom_str name) {
    bloom_enum_Result_bool_str r;
    if (name.len == 0 || name.len > BLOOM_ENV_MAX_VALUE) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_msg("remove", "name must be 1..32767 bytes");
        return r;
    }
    bloom_env_empty_remove(name);
    wchar_t *wname = bloom_env_to_utf16(name);
    if (wname == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_msg("remove", "name is not valid UTF-8");
        return r;
    }
    BOOL ok = SetEnvironmentVariableW(wname, NULL);
    DWORD err = ok ? 0 : GetLastError();
    free(wname);
    if (!ok) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_werr("remove", err);
        return r;
    }
    r.tag = 0;
    r.data.m_Ok._0 = true;
    return r;
}

bloom_enum_Result_str_str bloom_env_current_dir(void) {
    bloom_enum_Result_str_str r;
    DWORD need = GetCurrentDirectoryW(0, NULL);
    if (need == 0) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_werr("current_dir", GetLastError());
        return r;
    }
    wchar_t *buf = (wchar_t *)malloc((size_t)need * sizeof(wchar_t));
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    DWORD got = GetCurrentDirectoryW(need, buf);
    if (got == 0 || got >= need) {
        free(buf);
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_werr("current_dir", GetLastError());
        return r;
    }
    r.tag = 0;
    r.data.m_Ok._0 = bloom_env_from_utf16(buf, (int)got);
    free(buf);
    return r;
}

bloom_enum_Result_bool_str bloom_env_set_current_dir(bloom_str path) {
    bloom_enum_Result_bool_str r;
    if (path.len == 0) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_msg("set_current_dir", "path must not be empty");
        return r;
    }
    wchar_t *wpath = bloom_env_to_utf16(path);
    if (wpath == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_msg("set_current_dir", "path is not valid UTF-8");
        return r;
    }
    BOOL ok = SetCurrentDirectoryW(wpath);
    DWORD err = ok ? 0 : GetLastError();
    free(wpath);
    if (!ok) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_env_werr("set_current_dir", err);
        return r;
    }
    r.tag = 0;
    r.data.m_Ok._0 = true;
    return r;
}

bloom_enum_Result_str_str bloom_env_executable_path(void) {
    bloom_enum_Result_str_str r;
    DWORD cap = 1024;
    for (;;) {
        wchar_t *buf = (wchar_t *)malloc((size_t)cap * sizeof(wchar_t));
        if (buf == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        DWORD got = GetModuleFileNameW(NULL, buf, cap);
        if (got == 0) {
            free(buf);
            r.tag = 1;
            r.data.m_Err._0 = bloom_env_werr("executable_path", GetLastError());
            return r;
        }
        if (got < cap) {
            r.tag = 0;
            r.data.m_Ok._0 = bloom_env_from_utf16(buf, (int)got);
            free(buf);
            return r;
        }
        free(buf);
        if (cap >= 32768) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_env_msg(
                "executable_path",
                "path exceeds the Windows limit"
            );
            return r;
        }
        cap *= 2;
    }
}

bloom_str bloom_env_os(void) {
    return bloom_str_from_bytes("windows", 7);
}

bloom_str bloom_env_arch(void) {
#if defined(_M_X64) || defined(__x86_64__)
    return bloom_str_from_bytes("x86_64", 6);
#elif defined(_M_ARM64) || defined(__aarch64__)
    return bloom_str_from_bytes("aarch64", 7);
#elif defined(_M_IX86) || defined(__i386__)
    return bloom_str_from_bytes("x86", 3);
#else
    return bloom_str_from_bytes("unknown", 7);
#endif
}

int64_t bloom_env_process_id(void) {
    return (int64_t)GetCurrentProcessId();
}

#else /* !_WIN32 */

#include <unistd.h>
#include <limits.h>

extern char **environ;

bloom_arr_str bloom_env_args(void) {
    bloom_arr_str out = bloom_arr_str_new();
    FILE *f = fopen("/proc/self/cmdline", "rb");
    if (f == NULL) {
        return out;
    }
    char buf[4096];
    size_t got = fread(buf, 1, sizeof buf, f);
    fclose(f);
    size_t start = 0;
    for (size_t i = 0; i < got; i++) {
        if (buf[i] == '\0') {
            bloom_arr_str_push_owned(
                out,
                bloom_str_from_bytes(buf + start, i - start)
            );
            start = i + 1;
            if (out.len >= BLOOM_ENV_MAX_ARGS) {
                break;
            }
        }
    }
    return out;
}

bloom_enum_Option_str bloom_env_get(bloom_str name) {
    bloom_enum_Option_str r;
    char *n = (char *)malloc(name.len + 1);
    if (n == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(n, name.data, name.len);
    n[name.len] = '\0';
    const char *v = getenv(n);
    free(n);
    if (v == NULL) {
        r.tag = 1;
        return r;
    }
    r.tag = 0;
    r.data.m_Some._0 = bloom_str_from_bytes(v, strlen(v));
    return r;
}

bool bloom_env_has(bloom_str name) {
    bloom_enum_Option_str o = bloom_env_get(name);
    bool found = o.tag == 0;
    if (found) {
        bloom_str_drop(&o.data.m_Some._0);
    }
    return found;
}

bloom_enum_Result_bool_str bloom_env_set(bloom_str name, bloom_str value) {
    bloom_enum_Result_bool_str r;
    char *n = (char *)malloc(name.len + 2);
    char *v = (char *)malloc(value.len + 1);
    if (n == NULL || v == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(n, name.data, name.len);
    n[name.len] = '\0';
    memcpy(v, value.data, value.len);
    v[value.len] = '\0';
    int ok = setenv(n, v, 1) == 0;
    free(n);
    free(v);
    r.tag = ok ? 0 : 1;
    if (ok) {
        r.data.m_Ok._0 = true;
    } else {
        r.data.m_Err._0 = bloom_str_from_bytes("set: failed", 12);
    }
    return r;
}

bloom_enum_Result_bool_str bloom_env_remove(bloom_str name) {
    bloom_enum_Result_bool_str r;
    char *n = (char *)malloc(name.len + 1);
    if (n == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(n, name.data, name.len);
    n[name.len] = '\0';
    int ok = unsetenv(n) == 0;
    free(n);
    r.tag = ok ? 0 : 1;
    if (ok) {
        r.data.m_Ok._0 = true;
    } else {
        r.data.m_Err._0 = bloom_str_from_bytes("remove: failed", 15);
    }
    return r;
}

bloom_enum_Result_str_str bloom_env_current_dir(void) {
    bloom_enum_Result_str_str r;
    char buf[PATH_MAX];
    if (getcwd(buf, sizeof buf) == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_str_from_bytes("current_dir: failed", 19);
        return r;
    }
    r.tag = 0;
    r.data.m_Ok._0 = bloom_str_from_bytes(buf, strlen(buf));
    return r;
}

bloom_enum_Result_bool_str bloom_env_set_current_dir(bloom_str path) {
    bloom_enum_Result_bool_str r;
    char *p = (char *)malloc(path.len + 1);
    if (p == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(p, path.data, path.len);
    p[path.len] = '\0';
    int ok = chdir(p) == 0;
    free(p);
    r.tag = ok ? 0 : 1;
    if (ok) {
        r.data.m_Ok._0 = true;
    } else {
        r.data.m_Err._0 = bloom_str_from_bytes("set_current_dir: failed", 22);
    }
    return r;
}

bloom_enum_Result_str_str bloom_env_executable_path(void) {
    bloom_enum_Result_str_str r;
    char buf[PATH_MAX];
    ssize_t got = readlink("/proc/self/exe", buf, sizeof buf - 1);
    if (got < 0) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_str_from_bytes("executable_path: failed", 23);
        return r;
    }
    r.tag = 0;
    r.data.m_Ok._0 = bloom_str_from_bytes(buf, (size_t)got);
    return r;
}

bloom_str bloom_env_os(void) {
#if defined(__APPLE__)
    return bloom_str_from_bytes("macos", 5);
#else
    return bloom_str_from_bytes("linux", 5);
#endif
}

bloom_str bloom_env_arch(void) {
#if defined(__x86_64__)
    return bloom_str_from_bytes("x86_64", 6);
#elif defined(__aarch64__)
    return bloom_str_from_bytes("aarch64", 7);
#else
    return bloom_str_from_bytes("unknown", 7);
#endif
}

int64_t bloom_env_process_id(void) {
    return (int64_t)getpid();
}

#endif
