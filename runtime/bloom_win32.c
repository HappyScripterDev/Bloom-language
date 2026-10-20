/* Bloom std::win32 runtime — selected Windows API wrappers.
 *
 * Only emitted (and only registered by the checker) on Windows. All
 * functions are safe Bloom APIs: no raw handles or pointers escape, and
 * UTF-16 APIs are used with proper UTF-8 conversion.
 */

#ifdef _WIN32

static wchar_t *bloom_win32_to_utf16(bloom_str s) {
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

static bloom_str bloom_win32_from_utf16(const wchar_t *w, int wlen) {
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

static bloom_str bloom_win32_err_msg(const char *op, DWORD code) {
    char buf[128];
    snprintf(buf, sizeof buf, "%s: Windows error %lu", op, (unsigned long)code);
    return bloom_str_from_bytes(buf, strlen(buf));
}

static bloom_enum_Result_str_str bloom_win32_ok_str(bloom_str v) {
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_str_str bloom_win32_err_str(bloom_str m) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_i64_str bloom_win32_ok_i64(int64_t v) {
    bloom_enum_Result_i64_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_i64_str bloom_win32_err_i64(bloom_str m) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

int64_t bloom_win32_process_id(void) {
    return (int64_t)GetCurrentProcessId();
}

int64_t bloom_win32_tick_count_ms(void) {
    return (int64_t)GetTickCount64();
}

int64_t bloom_win32_last_error(void) {
    return (int64_t)GetLastError();
}

bloom_enum_Result_str_str bloom_win32_get_computer_name(void) {
    wchar_t buf[256];
    DWORD size = (DWORD)(sizeof buf / sizeof buf[0]);
    if (!GetComputerNameW(buf, &size)) {
        return bloom_win32_err_str(bloom_win32_err_msg("get_computer_name", GetLastError()));
    }
    return bloom_win32_ok_str(bloom_win32_from_utf16(buf, (int)size));
}

/* Sleeps for the given number of milliseconds (chunked for values above the
 * platform limit). Zero is a no-op; negative durations are a runtime error. */
void bloom_win32_sleep_ms(int64_t ms) {
    if (ms < 0) {
        bloom_panicf(NULL, 0, "sleep_ms: negative duration (%" PRId64 ")", ms);
    }
    while (ms > 0) {
        DWORD chunk = (ms > (int64_t)0x7FFFFFFF) ? 0x7FFFFFFFu : (DWORD)ms;
        Sleep(chunk);
        ms -= (int64_t)chunk;
    }
}

/* Shows a modal message box. `kind`: 0 = info, 1 = warning, 2 = error,
 * 3 = question. Returns the button ID chosen by the user. */
bloom_enum_Result_i64_str bloom_win32_message_box(
    bloom_str title,
    bloom_str message,
    int64_t kind
) {
    UINT flags = 0;
    switch (kind) {
        case 0:
            flags = MB_OK | MB_ICONINFORMATION;
            break;
        case 1:
            flags = MB_OK | MB_ICONWARNING;
            break;
        case 2:
            flags = MB_OK | MB_ICONERROR;
            break;
        case 3:
            flags = MB_YESNO | MB_ICONQUESTION;
            break;
        default:
            return bloom_win32_err_i64(bloom_win32_err_msg(
                "message_box: unknown kind",
                (DWORD)kind
            ));
    }
    wchar_t *wt = bloom_win32_to_utf16(title);
    wchar_t *wm = bloom_win32_to_utf16(message);
    if (wt == NULL || wm == NULL) {
        free(wt);
        free(wm);
        return bloom_win32_err_i64(bloom_str_from_bytes(
            "message_box: invalid UTF-8 in title or message",
            49
        ));
    }
    int result = MessageBoxW(NULL, wm, wt, flags);
    free(wt);
    free(wm);
    if (result == 0) {
        return bloom_win32_err_i64(bloom_win32_err_msg("message_box", GetLastError()));
    }
    return bloom_win32_ok_i64((int64_t)result);
}

#endif /* _WIN32 */
