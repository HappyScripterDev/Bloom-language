/* Bloom std::fs runtime — file and directory operations.
 *
 * Only emitted when the program loads `std::fs`. On Windows all paths are
 * converted from UTF-8 to UTF-16 and passed to the wide-character APIs, so
 * non-ASCII file names work. On POSIX the UTF-8 bytes are used directly.
 *
 * Error contract: functions return `Result<T, string>`; the error string
 * names the operation, the path and a short reason. No filesystem error is
 * silently turned into a plain `false`.
 */

#include <errno.h>
#ifndef _WIN32
#include <sys/stat.h>
#include <dirent.h>
#include <unistd.h>
#endif

/* ---- shared helpers ---------------------------------------------------- */

static bool bloom_fs_utf8_valid(const unsigned char *s, size_t n) {
    size_t i = 0;
    while (i < n) {
        uint32_t cp = 0;
        int used = bloom_utf8_decode(s + i, n - i, &cp);
        if (used <= 0) {
            return false;
        }
        i += (size_t)used;
    }
    return true;
}

static bloom_str bloom_fs_err_msg(const char *op, bloom_str path, const char *reason) {
    size_t opn = strlen(op);
    size_t rn = strlen(reason);
    size_t total = opn + 2 + path.len + 2 + rn;
    char *buf = (char *)malloc(total + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t at = 0;
    memcpy(buf + at, op, opn);
    at += opn;
    buf[at++] = ':';
    buf[at++] = ' ';
    if (path.len > 0) {
        memcpy(buf + at, path.data, path.len);
        at += path.len;
    }
    buf[at++] = ':';
    buf[at++] = ' ';
    memcpy(buf + at, reason, rn);
    at += rn;
    bloom_str msg = bloom_str_from_bytes(buf, at);
    free(buf);
    return msg;
}

/* Result constructors for the concrete instantiations used by std::fs. */
static bloom_enum_Result_str_str bloom_fs_ok_str(bloom_str v) {
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_str_str bloom_fs_err_str(bloom_str m) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_i64_str bloom_fs_ok_i64(int64_t v) {
    bloom_enum_Result_i64_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_i64_str bloom_fs_err_i64(bloom_str m) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_bool_str bloom_fs_ok_bool(bool v) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_bool_str bloom_fs_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_arr_str_str bloom_fs_ok_arr(bloom_arr_str v) {
    bloom_enum_Result_arr_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_arr_str_str bloom_fs_err_arr(bloom_str m) {
    bloom_enum_Result_arr_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

#ifdef _WIN32

static wchar_t *bloom_fs_to_utf16(bloom_str s) {
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

static bloom_str bloom_fs_from_utf16(const wchar_t *w, int wlen) {
    int need = WideCharToMultiByte(CP_UTF8, 0, w, wlen, NULL, 0, NULL, NULL);
    if (need < 0) {
        need = 0;
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

static const char *bloom_fs_win_reason(DWORD e) {
    switch (e) {
        case ERROR_FILE_NOT_FOUND:
            return "file not found";
        case ERROR_PATH_NOT_FOUND:
            return "path not found";
        case ERROR_ACCESS_DENIED:
            return "access denied";
        case ERROR_ALREADY_EXISTS:
            return "already exists";
        case ERROR_FILE_EXISTS:
            return "file already exists";
        case ERROR_DIRECTORY:
            return "not a directory";
        case ERROR_SHARING_VIOLATION:
            return "file is in use";
        case ERROR_DISK_FULL:
            return "disk full";
        case ERROR_NOT_ENOUGH_MEMORY:
            return "not enough memory";
        case ERROR_INVALID_NAME:
            return "invalid path";
        case ERROR_WRITE_PROTECT:
            return "write protected";
        default:
            return NULL;
    }
}

static bloom_str bloom_fs_win_err(const char *op, bloom_str path, DWORD e) {
    const char *reason = bloom_fs_win_reason(e);
    char num[48];
    if (reason == NULL) {
        snprintf(num, sizeof num, "system error %lu", (unsigned long)e);
        reason = num;
    }
    return bloom_fs_err_msg(op, path, reason);
}

#else

static bloom_str bloom_fs_posix_err(const char *op, bloom_str path, int e) {
    return bloom_fs_err_msg(op, path, strerror(e));
}

#endif

/* ---- reading and writing ---------------------------------------------- */

bloom_enum_Result_str_str bloom_fs_read_text(bloom_str path) {
    FILE *f = NULL;
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_str(bloom_fs_err_msg("read_text", path, "invalid UTF-8 in path"));
    }
    f = _wfopen(w, L"rb");
    if (f == NULL) {
        bloom_str m = bloom_fs_win_err("read_text", path, GetLastError());
        free(w);
        return bloom_fs_err_str(m);
    }
    free(w);
#else
    f = fopen(path.data, "rb");
    if (f == NULL) {
        return bloom_fs_err_str(bloom_fs_posix_err("read_text", path, errno));
    }
#endif
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return bloom_fs_err_str(bloom_fs_err_msg("read_text", path, "seek failed"));
    }
    long size = ftell(f);
    if (size < 0) {
        fclose(f);
        return bloom_fs_err_str(bloom_fs_err_msg("read_text", path, "seek failed"));
    }
    if (fseek(f, 0, SEEK_SET) != 0) {
        fclose(f);
        return bloom_fs_err_str(bloom_fs_err_msg("read_text", path, "seek failed"));
    }
    char *buf = (char *)malloc((size_t)size + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t got = fread(buf, 1, (size_t)size, f);
    int close_failed = fclose(f) != 0;
    if (got != (size_t)size || close_failed) {
        free(buf);
        return bloom_fs_err_str(bloom_fs_err_msg("read_text", path, "read failed"));
    }
    if (!bloom_fs_utf8_valid((const unsigned char *)buf, got)) {
        free(buf);
        return bloom_fs_err_str(bloom_fs_err_msg("read_text", path, "file is not valid UTF-8"));
    }
    bloom_str s = bloom_str_from_bytes(buf, got);
    free(buf);
    return bloom_fs_ok_str(s);
}

static bloom_enum_Result_i64_str bloom_fs_write_impl(
    const char *op,
    bloom_str path,
    bloom_str content,
    const char *mode,
    const wchar_t *wmode
) {
    FILE *f = NULL;
    (void)wmode;
    (void)mode;
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_i64(bloom_fs_err_msg(op, path, "invalid UTF-8 in path"));
    }
    f = _wfopen(w, wmode);
    if (f == NULL) {
        bloom_str m = bloom_fs_win_err(op, path, GetLastError());
        free(w);
        return bloom_fs_err_i64(m);
    }
    free(w);
#else
    f = fopen(path.data, mode);
    if (f == NULL) {
        return bloom_fs_err_i64(bloom_fs_posix_err(op, path, errno));
    }
#endif
    size_t wrote = 0;
    if (content.len > 0) {
        wrote = fwrite(content.data, 1, content.len, f);
    }
    int close_failed = fclose(f) != 0;
    if (wrote != content.len || close_failed) {
        return bloom_fs_err_i64(bloom_fs_err_msg(op, path, "write failed"));
    }
    return bloom_fs_ok_i64((int64_t)wrote);
}

bloom_enum_Result_i64_str bloom_fs_write_text(bloom_str path, bloom_str content) {
    return bloom_fs_write_impl("write_text", path, content, "wb", L"wb");
}

bloom_enum_Result_i64_str bloom_fs_append_text(bloom_str path, bloom_str content) {
    return bloom_fs_write_impl("append_text", path, content, "ab", L"ab");
}

/* ---- inspection -------------------------------------------------------- */

bloom_enum_Result_bool_str bloom_fs_exists(bloom_str path) {
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_bool(bloom_fs_err_msg("exists", path, "invalid UTF-8 in path"));
    }
    DWORD attr = GetFileAttributesW(w);
    free(w);
    return bloom_fs_ok_bool(attr != INVALID_FILE_ATTRIBUTES);
#else
    struct stat st;
    return bloom_fs_ok_bool(stat(path.data, &st) == 0);
#endif
}

bloom_enum_Result_bool_str bloom_fs_is_file(bloom_str path) {
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_bool(bloom_fs_err_msg("is_file", path, "invalid UTF-8 in path"));
    }
    DWORD attr = GetFileAttributesW(w);
    free(w);
    if (attr == INVALID_FILE_ATTRIBUTES) {
        return bloom_fs_ok_bool(false);
    }
    return bloom_fs_ok_bool((attr & FILE_ATTRIBUTE_DIRECTORY) == 0);
#else
    struct stat st;
    if (stat(path.data, &st) != 0) {
        return bloom_fs_ok_bool(false);
    }
    return bloom_fs_ok_bool(S_ISREG(st.st_mode));
#endif
}

bloom_enum_Result_bool_str bloom_fs_is_dir(bloom_str path) {
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_bool(bloom_fs_err_msg("is_dir", path, "invalid UTF-8 in path"));
    }
    DWORD attr = GetFileAttributesW(w);
    free(w);
    if (attr == INVALID_FILE_ATTRIBUTES) {
        return bloom_fs_ok_bool(false);
    }
    return bloom_fs_ok_bool((attr & FILE_ATTRIBUTE_DIRECTORY) != 0);
#else
    struct stat st;
    if (stat(path.data, &st) != 0) {
        return bloom_fs_ok_bool(false);
    }
    return bloom_fs_ok_bool(S_ISDIR(st.st_mode));
#endif
}

bloom_enum_Result_i64_str bloom_fs_file_size(bloom_str path) {
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_i64(bloom_fs_err_msg("file_size", path, "invalid UTF-8 in path"));
    }
    WIN32_FILE_ATTRIBUTE_DATA info;
    if (!GetFileAttributesExW(w, GetFileExInfoStandard, &info)) {
        bloom_str m = bloom_fs_win_err("file_size", path, GetLastError());
        free(w);
        return bloom_fs_err_i64(m);
    }
    free(w);
    if ((info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0) {
        return bloom_fs_err_i64(bloom_fs_err_msg("file_size", path, "is a directory"));
    }
    int64_t size = ((int64_t)info.nFileSizeHigh << 32) | (int64_t)info.nFileSizeLow;
    return bloom_fs_ok_i64(size);
#else
    struct stat st;
    if (stat(path.data, &st) != 0) {
        return bloom_fs_err_i64(bloom_fs_posix_err("file_size", path, errno));
    }
    if (S_ISDIR(st.st_mode)) {
        return bloom_fs_err_i64(bloom_fs_err_msg("file_size", path, "is a directory"));
    }
    return bloom_fs_ok_i64((int64_t)st.st_size);
#endif
}

/* ---- directories ------------------------------------------------------- */

#ifdef _WIN32

static bool bloom_fs_mkdirs_w(const wchar_t *w, DWORD *err) {
    size_t len = wcslen(w);
    if (len == 0) {
        *err = ERROR_PATH_NOT_FOUND;
        return false;
    }
    size_t start = 1;
    if (len >= 2 && (w[0] == L'\\' || w[0] == L'/') && (w[1] == L'\\' || w[1] == L'/')) {
        /* UNC path: skip past the share name (\\server\share). */
        size_t sep = 0;
        for (size_t i = 2; i < len; i++) {
            if (w[i] == L'\\' || w[i] == L'/') {
                sep++;
                if (sep == 2) {
                    start = i + 1;
                    break;
                }
            }
        }
        if (sep < 2) {
            start = len;
        }
    }
    wchar_t *tmp = (wchar_t *)malloc((len + 1) * sizeof(wchar_t));
    if (tmp == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(tmp, w, (len + 1) * sizeof(wchar_t));
    for (size_t i = start; i <= len; i++) {
        if (i == len || tmp[i] == L'\\' || tmp[i] == L'/') {
            wchar_t save = tmp[i];
            tmp[i] = L'\0';
            if (i > 0 && !(i == 2 && tmp[1] == L':')) {
                if (!CreateDirectoryW(tmp, NULL)) {
                    DWORD e = GetLastError();
                    if (e != ERROR_ALREADY_EXISTS) {
                        *err = e;
                        free(tmp);
                        return false;
                    }
                    DWORD attr = GetFileAttributesW(tmp);
                    if (attr == INVALID_FILE_ATTRIBUTES || (attr & FILE_ATTRIBUTE_DIRECTORY) == 0) {
                        *err = ERROR_DIRECTORY;
                        free(tmp);
                        return false;
                    }
                }
            }
            tmp[i] = save;
        }
    }
    free(tmp);
    return true;
}

#endif

bloom_enum_Result_bool_str bloom_fs_create_dir_all(bloom_str path) {
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_bool(bloom_fs_err_msg(
            "create_dir_all",
            path,
            "invalid UTF-8 in path"
        ));
    }
    DWORD err = 0;
    if (!bloom_fs_mkdirs_w(w, &err)) {
        bloom_str m = bloom_fs_win_err("create_dir_all", path, err);
        free(w);
        return bloom_fs_err_bool(m);
    }
    free(w);
    return bloom_fs_ok_bool(true);
#else
    size_t len = path.len;
    if (len == 0) {
        return bloom_fs_err_bool(bloom_fs_err_msg(
            "create_dir_all",
            path,
            "empty path"
        ));
    }
    char *tmp = (char *)malloc(len + 1);
    if (tmp == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(tmp, path.data, len);
    tmp[len] = '\0';
    for (size_t i = 1; i <= len; i++) {
        if (i == len || tmp[i] == '/') {
            char save = tmp[i];
            tmp[i] = '\0';
            if (mkdir(tmp, 0777) != 0 && errno != EEXIST) {
                bloom_str m = bloom_fs_posix_err("create_dir_all", path, errno);
                free(tmp);
                return bloom_fs_err_bool(m);
            }
            tmp[i] = save;
        }
    }
    free(tmp);
    return bloom_fs_ok_bool(true);
#endif
}

bloom_enum_Result_bool_str bloom_fs_remove_file(bloom_str path) {
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_bool(bloom_fs_err_msg(
            "remove_file",
            path,
            "invalid UTF-8 in path"
        ));
    }
    if (!DeleteFileW(w)) {
        bloom_str m = bloom_fs_win_err("remove_file", path, GetLastError());
        free(w);
        return bloom_fs_err_bool(m);
    }
    free(w);
    return bloom_fs_ok_bool(true);
#else
    if (unlink(path.data) != 0) {
        return bloom_fs_err_bool(bloom_fs_posix_err("remove_file", path, errno));
    }
    return bloom_fs_ok_bool(true);
#endif
}

bloom_enum_Result_bool_str bloom_fs_remove_dir(bloom_str path) {
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_bool(bloom_fs_err_msg(
            "remove_dir",
            path,
            "invalid UTF-8 in path"
        ));
    }
    if (!RemoveDirectoryW(w)) {
        bloom_str m = bloom_fs_win_err("remove_dir", path, GetLastError());
        free(w);
        return bloom_fs_err_bool(m);
    }
    free(w);
    return bloom_fs_ok_bool(true);
#else
    if (rmdir(path.data) != 0) {
        return bloom_fs_err_bool(bloom_fs_posix_err("remove_dir", path, errno));
    }
    return bloom_fs_ok_bool(true);
#endif
}

bloom_enum_Result_i64_str bloom_fs_copy_file(bloom_str source, bloom_str destination) {
#ifdef _WIN32
    wchar_t *ws = bloom_fs_to_utf16(source);
    wchar_t *wd = bloom_fs_to_utf16(destination);
    if (ws == NULL || wd == NULL) {
        free(ws);
        free(wd);
        return bloom_fs_err_i64(bloom_fs_err_msg(
            "copy_file",
            source,
            "invalid UTF-8 in path"
        ));
    }
    WIN32_FILE_ATTRIBUTE_DATA info;
    if (!GetFileAttributesExW(ws, GetFileExInfoStandard, &info)) {
        bloom_str m = bloom_fs_win_err("copy_file", source, GetLastError());
        free(ws);
        free(wd);
        return bloom_fs_err_i64(m);
    }
    if ((info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0) {
        free(ws);
        free(wd);
        return bloom_fs_err_i64(bloom_fs_err_msg("copy_file", source, "is a directory"));
    }
    if (!CopyFileW(ws, wd, FALSE)) {
        bloom_str m = bloom_fs_win_err("copy_file", destination, GetLastError());
        free(ws);
        free(wd);
        return bloom_fs_err_i64(m);
    }
    free(ws);
    free(wd);
    int64_t size = ((int64_t)info.nFileSizeHigh << 32) | (int64_t)info.nFileSizeLow;
    return bloom_fs_ok_i64(size);
#else
    FILE *in = fopen(source.data, "rb");
    if (in == NULL) {
        return bloom_fs_err_i64(bloom_fs_posix_err("copy_file", source, errno));
    }
    FILE *out = fopen(destination.data, "wb");
    if (out == NULL) {
        fclose(in);
        return bloom_fs_err_i64(bloom_fs_posix_err("copy_file", destination, errno));
    }
    char buf[65536];
    int64_t total = 0;
    size_t got;
    while ((got = fread(buf, 1, sizeof buf, in)) > 0) {
        if (fwrite(buf, 1, got, out) != got) {
            fclose(in);
            fclose(out);
            return bloom_fs_err_i64(bloom_fs_err_msg("copy_file", destination, "write failed"));
        }
        total += (int64_t)got;
    }
    int bad = ferror(in);
    fclose(in);
    if (fclose(out) != 0) {
        bad = 1;
    }
    if (bad) {
        return bloom_fs_err_i64(bloom_fs_err_msg("copy_file", source, "read failed"));
    }
    return bloom_fs_ok_i64(total);
#endif
}

bloom_enum_Result_bool_str bloom_fs_rename(bloom_str source, bloom_str destination) {
#ifdef _WIN32
    wchar_t *ws = bloom_fs_to_utf16(source);
    wchar_t *wd = bloom_fs_to_utf16(destination);
    if (ws == NULL || wd == NULL) {
        free(ws);
        free(wd);
        return bloom_fs_err_bool(bloom_fs_err_msg(
            "rename",
            source,
            "invalid UTF-8 in path"
        ));
    }
    if (!MoveFileExW(ws, wd, MOVEFILE_REPLACE_EXISTING)) {
        bloom_str m = bloom_fs_win_err("rename", source, GetLastError());
        free(ws);
        free(wd);
        return bloom_fs_err_bool(m);
    }
    free(ws);
    free(wd);
    return bloom_fs_ok_bool(true);
#else
    if (rename(source.data, destination.data) != 0) {
        return bloom_fs_err_bool(bloom_fs_posix_err("rename", source, errno));
    }
    return bloom_fs_ok_bool(true);
#endif
}

bloom_enum_Result_str_str bloom_fs_current_dir(void) {
#ifdef _WIN32
    DWORD need = GetCurrentDirectoryW(0, NULL);
    if (need == 0) {
        return bloom_fs_err_str(bloom_fs_err_msg(
            "current_dir",
            (bloom_str){0},
            "could not read working directory"
        ));
    }
    wchar_t *w = (wchar_t *)malloc(((size_t)need + 1) * sizeof(wchar_t));
    if (w == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    DWORD got = GetCurrentDirectoryW(need + 1, w);
    if (got == 0) {
        free(w);
        return bloom_fs_err_str(bloom_fs_err_msg(
            "current_dir",
            (bloom_str){0},
            "could not read working directory"
        ));
    }
    bloom_str s = bloom_fs_from_utf16(w, (int)got);
    free(w);
    return bloom_fs_ok_str(s);
#else
    char buf[4096];
    if (getcwd(buf, sizeof buf) == NULL) {
        return bloom_fs_err_str(bloom_fs_posix_err(
            "current_dir",
            (bloom_str){0},
            errno
        ));
    }
    return bloom_fs_ok_str(bloom_str_from_bytes(buf, strlen(buf)));
#endif
}

bloom_str bloom_fs_join_path(bloom_str base, bloom_str child) {
    if (child.len > 0) {
        bool child_absolute = false;
#ifdef _WIN32
        if (child.len >= 2 && child.data[1] == ':') {
            child_absolute = true;
        }
        if (child.len >= 2 && (child.data[0] == '\\' || child.data[0] == '/')
            && (child.data[1] == '\\' || child.data[1] == '/')) {
            child_absolute = true;
        }
#else
        if (child.data[0] == '/') {
            child_absolute = true;
        }
#endif
        if (child_absolute) {
            return bloom_str_from_bytes(child.data, child.len);
        }
    }
    if (base.len == 0) {
        return bloom_str_from_bytes(child.data, child.len);
    }
#ifdef _WIN32
    char sep = '\\';
    bool base_has_sep = base.data[base.len - 1] == '\\' || base.data[base.len - 1] == '/';
#else
    char sep = '/';
    bool base_has_sep = base.data[base.len - 1] == '/';
#endif
    size_t total = base.len + (base_has_sep ? 0 : 1) + child.len;
    char *buf = (char *)malloc(total + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(buf, base.data, base.len);
    size_t at = base.len;
    if (!base_has_sep) {
        buf[at++] = sep;
    }
    if (child.len > 0) {
        memcpy(buf + at, child.data, child.len);
        at += child.len;
    }
    bloom_str s = bloom_str_from_bytes(buf, at);
    free(buf);
    return s;
}

/* ---- directory listing ------------------------------------------------ */

static int bloom_fs_str_cmp(const void *a, const void *b) {
    const bloom_str *x = (const bloom_str *)a;
    const bloom_str *y = (const bloom_str *)b;
    size_t n = x->len < y->len ? x->len : y->len;
    int c = memcmp(x->data, y->data, n);
    if (c != 0) {
        return c;
    }
    if (x->len == y->len) {
        return 0;
    }
    return x->len < y->len ? -1 : 1;
}

bloom_enum_Result_arr_str_str bloom_fs_read_dir(bloom_str path) {
    bloom_arr_str result = bloom_arr_str_new();
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        bloom_arr_str_drop(&result);
        return bloom_fs_err_arr(bloom_fs_err_msg(
            "read_dir",
            path,
            "invalid UTF-8 in path"
        ));
    }
    size_t len = wcslen(w);
    wchar_t *pattern = (wchar_t *)malloc((len + 3) * sizeof(wchar_t));
    if (pattern == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(pattern, w, len * sizeof(wchar_t));
    if (len > 0 && pattern[len - 1] != L'\\' && pattern[len - 1] != L'/') {
        pattern[len] = L'\\';
        pattern[len + 1] = L'*';
        pattern[len + 2] = L'\0';
    } else {
        pattern[len] = L'*';
        pattern[len + 1] = L'\0';
    }
    free(w);
    WIN32_FIND_DATAW data;
    HANDLE h = FindFirstFileW(pattern, &data);
    free(pattern);
    if (h == INVALID_HANDLE_VALUE) {
        DWORD e = GetLastError();
        bloom_arr_str_drop(&result);
        return bloom_fs_err_arr(bloom_fs_win_err("read_dir", path, e));
    }
    for (;;) {
        if (wcscmp(data.cFileName, L".") != 0 && wcscmp(data.cFileName, L"..") != 0) {
            bloom_str name = bloom_fs_from_utf16(data.cFileName, -1);
            bloom_arr_str_push_owned(result, name);
        }
        if (!FindNextFileW(h, &data)) {
            break;
        }
    }
    FindClose(h);
#else
    DIR *d = opendir(path.data);
    if (d == NULL) {
        bloom_arr_str_drop(&result);
        return bloom_fs_err_arr(bloom_fs_posix_err("read_dir", path, errno));
    }
    struct dirent *entry;
    while ((entry = readdir(d)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        bloom_arr_str_push_owned(result, bloom_str_from_bytes(entry->d_name, strlen(entry->d_name)));
    }
    closedir(d);
#endif
    if (result->len > 1) {
        qsort(result->data, result->len, sizeof(bloom_str), bloom_fs_str_cmp);
    }
    return bloom_fs_ok_arr(result);
}

/* ---- bounded reads, byte buffers and confined file access (0.15.0) ------ */

/* Upper bound for a single static web file (documented). */
#define BLOOM_FS_MAX_BYTES (8 * 1024 * 1024)

typedef struct {
    size_t rc;
    unsigned char *data;
    size_t len;
} bloom_fs_bytes;

bloom_fs_handle bloom_fs_bytes_retain(bloom_fs_handle h) {
    bloom_fs_bytes *b = (bloom_fs_bytes *)(intptr_t)h;
    if (b != NULL) {
        b->rc++;
    }
    return h;
}

void bloom_fs_bytes_release(bloom_fs_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_fs_bytes *b = (bloom_fs_bytes *)(intptr_t)*h;
    *h = 0;
    if (b->rc > 0) {
        b->rc--;
    }
    if (b->rc == 0) {
        free(b->data);
        free(b);
        bloom_live_allocs--;
    }
}

static bloom_struct_std_fs_Bytes bloom_fs_bytes_wrap(bloom_fs_bytes *b) {
    bloom_struct_std_fs_Bytes v;
    v.f_id = (bloom_fs_handle)(intptr_t)b;
    return v;
}

static bloom_fs_bytes *bloom_fs_bytes_unwrap(bloom_struct_std_fs_Bytes v) {
    return (bloom_fs_bytes *)(intptr_t)v.f_id;
}

static bloom_struct_std_fs_Bytes bloom_fs_bytes_take(unsigned char *data, size_t len) {
    bloom_fs_bytes *b = (bloom_fs_bytes *)calloc(1, sizeof *b);
    if (b == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    b->rc = 1;
    b->data = data;
    b->len = len;
    bloom_live_allocs++;
    return bloom_fs_bytes_wrap(b);
}

static bloom_enum_Result_std_fs_Bytes_str bloom_fs_ok_bytes(
    bloom_struct_std_fs_Bytes v
) {
    bloom_enum_Result_std_fs_Bytes_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_std_fs_Bytes_str bloom_fs_err_bytes(bloom_str m) {
    bloom_enum_Result_std_fs_Bytes_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

/* ---- path joining and confined opening ---------------------------------- */

typedef enum {
    BLOOM_FS_OPEN_OK = 0,
    BLOOM_FS_OPEN_NOT_FOUND = 1,
    BLOOM_FS_OPEN_DENIED = 2,
    BLOOM_FS_OPEN_TOO_LARGE = 3,
    BLOOM_FS_OPEN_OUTSIDE = 4,
    BLOOM_FS_OPEN_ERROR = 5
} bloom_fs_open_status;

/* Validates `rel` as a relative path (no absolute paths, drive or UNC
 * prefixes, `..` segments, empty segments, control bytes or NUL; on Windows
 * also no `:` inside a segment) and joins it with `root` using `/`.
 * `.` segments are dropped. */
static bool bloom_fs_join_inside_root(
    bloom_str root,
    bloom_str rel,
    char **out,
    size_t *out_len,
    bloom_str *err
) {
    if (root.len == 0) {
        *err = bloom_fs_err_msg("path", root, "empty resource root");
        return false;
    }
    if (rel.len == 0) {
        *err = bloom_fs_err_msg("path", rel, "empty relative path");
        return false;
    }
    char c0 = rel.data[0];
    bool drive = rel.len >= 2 && rel.data[1] == ':'
        && ((c0 >= 'a' && c0 <= 'z') || (c0 >= 'A' && c0 <= 'Z'));
    if (c0 == '/' || c0 == '\\' || drive) {
        *err = bloom_fs_err_msg("path", rel, "absolute paths are not allowed");
        return false;
    }
    size_t rlen = root.len;
    while (rlen > 0 && (root.data[rlen - 1] == '/' || root.data[rlen - 1] == '\\')) {
        rlen--;
    }
    size_t cap = rlen + rel.len + 2;
    char *buf = (char *)malloc(cap);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t at = 0;
    if (rlen > 0) {
        memcpy(buf, root.data, rlen);
        at = rlen;
    } else {
        buf[at++] = '/';
    }
    bool any = false;
    size_t i = 0;
    while (i < rel.len) {
        size_t start = i;
        while (i < rel.len && rel.data[i] != '/' && rel.data[i] != '\\') {
            i++;
        }
        size_t len = i - start;
        if (len == 0) {
            free(buf);
            *err = bloom_fs_err_msg("path", rel, "empty path segment");
            return false;
        }
        if (len == 2 && rel.data[start] == '.' && rel.data[start + 1] == '.') {
            free(buf);
            *err = bloom_fs_err_msg("path", rel, "`..` segments are not allowed");
            return false;
        }
        for (size_t k = 0; k < len; k++) {
            unsigned char c = (unsigned char)rel.data[start + k];
            if (c < 0x20) {
                free(buf);
                *err = bloom_fs_err_msg("path", rel, "control bytes are not allowed");
                return false;
            }
#ifdef _WIN32
            if (c == ':') {
                free(buf);
                *err = bloom_fs_err_msg("path", rel, "`:` is not allowed in a resource path");
                return false;
            }
#endif
        }
        if (!(len == 1 && rel.data[start] == '.')) {
            if (at + len + 1 > cap) {
                free(buf);
                *err = bloom_fs_err_msg("path", rel, "path is too long");
                return false;
            }
            buf[at++] = '/';
            memcpy(buf + at, rel.data + start, len);
            at += len;
            any = true;
        }
        i++;
    }
    if (!any) {
        free(buf);
        *err = bloom_fs_err_msg("path", rel, "path contains no file name");
        return false;
    }
    buf[at] = '\0';
    *out = buf;
    *out_len = at;
    return true;
}

/* Reads a whole file into a fresh buffer. */
static bool bloom_fs_read_all(FILE *f, size_t size, unsigned char **out, size_t *out_len) {
    unsigned char *buf = (unsigned char *)malloc(size ? size : 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t got = fread(buf, 1, size, f);
    if (got != size) {
        free(buf);
        return false;
    }
    *out = buf;
    *out_len = size;
    return true;
}

#ifdef _WIN32

/* Opens `path` and verifies that the opened file (including any resolved
 * symbolic links or junctions) lies inside `root`. The check runs on the
 * opened handle, so the verified object is the one that gets read. */
static bloom_fs_open_status bloom_fs_open_confined(
    bloom_str root,
    bloom_str path,
    int64_t max_bytes,
    FILE **out_file,
    size_t *out_size,
    bloom_str *err
) {
    wchar_t *wpath = bloom_fs_to_utf16(path);
    if (wpath == NULL) {
        *err = bloom_fs_err_msg("open", path, "invalid UTF-8 in path");
        return BLOOM_FS_OPEN_ERROR;
    }
    DWORD attrs = GetFileAttributesW(wpath);
    if (attrs == INVALID_FILE_ATTRIBUTES) {
        DWORD e = GetLastError();
        if (e == ERROR_FILE_NOT_FOUND || e == ERROR_PATH_NOT_FOUND) {
            free(wpath);
            *err = bloom_fs_err_msg("open", path, "file not found");
            return BLOOM_FS_OPEN_NOT_FOUND;
        }
        if (e == ERROR_ACCESS_DENIED) {
            free(wpath);
            *err = bloom_fs_err_msg("open", path, "access denied");
            return BLOOM_FS_OPEN_DENIED;
        }
        *err = bloom_fs_win_err("open", path, e);
        free(wpath);
        return BLOOM_FS_OPEN_ERROR;
    }
    if ((attrs & FILE_ATTRIBUTE_DIRECTORY) != 0) {
        free(wpath);
        *err = bloom_fs_err_msg("open", path, "is a directory");
        return BLOOM_FS_OPEN_NOT_FOUND;
    }
    FILE *f = _wfopen(wpath, L"rb");
    if (f == NULL) {
        int e = errno;
        free(wpath);
        if (e == ENOENT) {
            *err = bloom_fs_err_msg("open", path, "file not found");
            return BLOOM_FS_OPEN_NOT_FOUND;
        }
        if (e == EACCES) {
            *err = bloom_fs_err_msg("open", path, "access denied");
            return BLOOM_FS_OPEN_DENIED;
        }
        *err = bloom_fs_err_msg("open", path, strerror(e));
        return BLOOM_FS_OPEN_ERROR;
    }
    free(wpath);
    HANDLE h = (HANDLE)_get_osfhandle(_fileno(f));
    wchar_t final_path[32768];
    DWORD fn = GetFinalPathNameByHandleW(
        h,
        final_path,
        32768,
        FILE_NAME_NORMALIZED | VOLUME_NAME_DOS
    );
    if (fn == 0 || fn >= 32768) {
        fclose(f);
        *err = bloom_fs_err_msg("open", path, "could not resolve the final path");
        return BLOOM_FS_OPEN_ERROR;
    }
    wchar_t *wroot = bloom_fs_to_utf16(root);
    if (wroot == NULL) {
        fclose(f);
        *err = bloom_fs_err_msg("open", root, "invalid UTF-8 in resource root");
        return BLOOM_FS_OPEN_ERROR;
    }
    HANDLE rh = CreateFileW(
        wroot,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        NULL,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS,
        NULL
    );
    free(wroot);
    if (rh == INVALID_HANDLE_VALUE) {
        fclose(f);
        *err = bloom_fs_err_msg("open", root, "resource root is not accessible");
        return BLOOM_FS_OPEN_ERROR;
    }
    wchar_t root_final[32768];
    DWORD rn = GetFinalPathNameByHandleW(
        rh,
        root_final,
        32768,
        FILE_NAME_NORMALIZED | VOLUME_NAME_DOS
    );
    CloseHandle(rh);
    if (rn == 0 || rn >= 32768) {
        fclose(f);
        *err = bloom_fs_err_msg("open", root, "could not resolve the resource root");
        return BLOOM_FS_OPEN_ERROR;
    }
    bool inside = false;
    DWORD rlen = rn;
    while (rlen > 1 && root_final[rlen - 1] == L'\\') {
        rlen--;
    }
    if (rlen <= fn) {
        int cmp = CompareStringOrdinal(
            root_final,
            (int)rlen,
            final_path,
            (int)rlen,
            TRUE
        );
        if (cmp == CSTR_EQUAL) {
            if (fn == rlen) {
                inside = true;
            } else {
                wchar_t next = final_path[rlen];
                inside = next == L'\\' || next == L'/';
            }
        }
    }
    if (!inside) {
        fclose(f);
        *err = bloom_fs_err_msg(
            "open",
            path,
            "resolved path leaves the resource root"
        );
        return BLOOM_FS_OPEN_OUTSIDE;
    }
    LARGE_INTEGER size;
    if (!GetFileSizeEx(h, &size)) {
        fclose(f);
        *err = bloom_fs_err_msg("open", path, "could not determine the file size");
        return BLOOM_FS_OPEN_ERROR;
    }
    if (max_bytes > 0 && size.QuadPart > max_bytes) {
        fclose(f);
        char num[64];
        snprintf(num, sizeof num, "file exceeds %" PRId64 " bytes", max_bytes);
        *err = bloom_fs_err_msg("open", path, num);
        return BLOOM_FS_OPEN_TOO_LARGE;
    }
    *out_file = f;
    *out_size = (size_t)size.QuadPart;
    return BLOOM_FS_OPEN_OK;
}

#else

static bloom_fs_open_status bloom_fs_open_confined(
    bloom_str root,
    bloom_str path,
    int64_t max_bytes,
    FILE **out_file,
    size_t *out_size,
    bloom_str *err
) {
    struct stat st;
    if (stat(path.data, &st) != 0) {
        if (errno == ENOENT || errno == ENOTDIR) {
            *err = bloom_fs_err_msg("open", path, "file not found");
            return BLOOM_FS_OPEN_NOT_FOUND;
        }
        if (errno == EACCES) {
            *err = bloom_fs_err_msg("open", path, "access denied");
            return BLOOM_FS_OPEN_DENIED;
        }
        *err = bloom_fs_posix_err("open", path, errno);
        return BLOOM_FS_OPEN_ERROR;
    }
    if (S_ISDIR(st.st_mode)) {
        *err = bloom_fs_err_msg("open", path, "is a directory");
        return BLOOM_FS_OPEN_NOT_FOUND;
    }
    int fd = open(path.data, O_RDONLY);
    if (fd < 0) {
        if (errno == EACCES) {
            *err = bloom_fs_err_msg("open", path, "access denied");
            return BLOOM_FS_OPEN_DENIED;
        }
        *err = bloom_fs_posix_err("open", path, errno);
        return BLOOM_FS_OPEN_ERROR;
    }
    struct stat fst;
    if (fstat(fd, &fst) != 0) {
        close(fd);
        *err = bloom_fs_posix_err("open", path, errno);
        return BLOOM_FS_OPEN_ERROR;
    }
    char *real = realpath(path.data, NULL);
    char *root_real = realpath(root.data, NULL);
    if (real == NULL || root_real == NULL) {
        free(real);
        free(root_real);
        close(fd);
        *err = bloom_fs_err_msg("open", path, "could not resolve the final path");
        return BLOOM_FS_OPEN_ERROR;
    }
    struct stat rst;
    bool inside = false;
    if (stat(real, &rst) == 0 && rst.st_dev == fst.st_dev && rst.st_ino == fst.st_ino) {
        size_t rlen = strlen(root_real);
        while (rlen > 1 && root_real[rlen - 1] == '/') {
            rlen--;
        }
        if (strncmp(real, root_real, rlen) == 0) {
            if (real[rlen] == '\0') {
                inside = true;
            } else if (real[rlen] == '/') {
                inside = true;
            } else if (rlen == 1 && root_real[0] == '/') {
                inside = true;
            }
        }
    }
    free(real);
    free(root_real);
    if (!inside) {
        close(fd);
        *err = bloom_fs_err_msg(
            "open",
            path,
            "resolved path leaves the resource root"
        );
        return BLOOM_FS_OPEN_OUTSIDE;
    }
    if (max_bytes > 0 && fst.st_size > max_bytes) {
        close(fd);
        char num[64];
        snprintf(num, sizeof num, "file exceeds %" PRId64 " bytes", max_bytes);
        *err = bloom_fs_err_msg("open", path, num);
        return BLOOM_FS_OPEN_TOO_LARGE;
    }
    FILE *f = fdopen(fd, "rb");
    if (f == NULL) {
        close(fd);
        *err = bloom_fs_err_msg("open", path, "could not open the file stream");
        return BLOOM_FS_OPEN_ERROR;
    }
    *out_file = f;
    *out_size = (size_t)fst.st_size;
    return BLOOM_FS_OPEN_OK;
}

#endif

/* ---- public API ---------------------------------------------------------- */

bloom_enum_Result_str_str bloom_fs_read_text_limited(bloom_str path, int64_t limit) {
    if (limit < 1) {
        return bloom_fs_err_str(bloom_fs_err_msg(
            "read_text_limited",
            path,
            "limit must be positive"
        ));
    }
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_str(bloom_fs_err_msg(
            "read_text_limited",
            path,
            "invalid UTF-8 in path"
        ));
    }
    WIN32_FILE_ATTRIBUTE_DATA attrs;
    if (!GetFileAttributesExW(w, GetFileExInfoStandard, &attrs)) {
        DWORD e = GetLastError();
        free(w);
        return bloom_fs_err_str(bloom_fs_win_err("read_text_limited", path, e));
    }
    free(w);
    LARGE_INTEGER size;
    size.HighPart = (LONG)attrs.nFileSizeHigh;
    size.LowPart = attrs.nFileSizeLow;
    if (size.QuadPart > limit) {
        char num[64];
        snprintf(num, sizeof num, "file exceeds %" PRId64 " bytes", limit);
        return bloom_fs_err_str(bloom_fs_err_msg("read_text_limited", path, num));
    }
#else
    struct stat st;
    if (stat(path.data, &st) != 0) {
        return bloom_fs_err_str(bloom_fs_posix_err("read_text_limited", path, errno));
    }
    if (st.st_size > limit) {
        char num[64];
        snprintf(num, sizeof num, "file exceeds %" PRId64 " bytes", limit);
        return bloom_fs_err_str(bloom_fs_err_msg("read_text_limited", path, num));
    }
#endif
    return bloom_fs_read_text(path);
}

bloom_enum_Result_std_fs_Bytes_str bloom_fs_read_bytes(bloom_str path) {
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
#ifdef _WIN32
    wchar_t *w = bloom_fs_to_utf16(path);
    if (w == NULL) {
        return bloom_fs_err_bytes(bloom_fs_err_msg(
            "read_bytes",
            path,
            "invalid UTF-8 in path"
        ));
    }
    WIN32_FILE_ATTRIBUTE_DATA attrs;
    if (!GetFileAttributesExW(w, GetFileExInfoStandard, &attrs)) {
        DWORD e = GetLastError();
        free(w);
        return bloom_fs_err_bytes(bloom_fs_win_err("read_bytes", path, e));
    }
    if ((attrs.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0) {
        free(w);
        return bloom_fs_err_bytes(bloom_fs_err_msg("read_bytes", path, "is a directory"));
    }
    LARGE_INTEGER size_info;
    size_info.HighPart = (LONG)attrs.nFileSizeHigh;
    size_info.LowPart = attrs.nFileSizeLow;
    if (size_info.QuadPart > BLOOM_FS_MAX_BYTES) {
        char num[64];
        snprintf(num, sizeof num, "file exceeds %d bytes", BLOOM_FS_MAX_BYTES);
        free(w);
        return bloom_fs_err_bytes(bloom_fs_err_msg("read_bytes", path, num));
    }
    FILE *f = _wfopen(w, L"rb");
    free(w);
    if (f == NULL) {
        return bloom_fs_err_bytes(bloom_fs_err_msg(
            "read_bytes",
            path,
            strerror(errno)
        ));
    }
    size_t size = (size_t)size_info.QuadPart;
#else
    struct stat st;
    if (stat(path.data, &st) != 0) {
        return bloom_fs_err_bytes(bloom_fs_posix_err("read_bytes", path, errno));
    }
    if (S_ISDIR(st.st_mode)) {
        return bloom_fs_err_bytes(bloom_fs_err_msg("read_bytes", path, "is a directory"));
    }
    if (st.st_size > BLOOM_FS_MAX_BYTES) {
        char num[64];
        snprintf(num, sizeof num, "file exceeds %d bytes", BLOOM_FS_MAX_BYTES);
        return bloom_fs_err_bytes(bloom_fs_err_msg("read_bytes", path, num));
    }
    FILE *f = fopen(path.data, "rb");
    if (f == NULL) {
        return bloom_fs_err_bytes(bloom_fs_posix_err("read_bytes", path, errno));
    }
    size_t size = (size_t)st.st_size;
#endif
    unsigned char *data = NULL;
    size_t len = 0;
    bool ok = bloom_fs_read_all(f, size, &data, &len);
    fclose(f);
    if (!ok) {
        return bloom_fs_err_bytes(bloom_fs_err_msg(
            "read_bytes",
            path,
            "read failed"
        ));
    }
    return bloom_fs_ok_bytes(bloom_fs_bytes_take(data, len));
}

int64_t bloom_fs_bytes_len(bloom_struct_std_fs_Bytes b) {
    bloom_fs_bytes *bytes = bloom_fs_bytes_unwrap(b);
    if (bytes == NULL) {
        return 0;
    }
    return (int64_t)bytes->len;
}

