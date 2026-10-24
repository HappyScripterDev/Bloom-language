/* Bloom std::desktop runtime - native Win32 windows and controls.
 *
 * Only emitted when a program imports std::desktop. Windows 11 is the
 * supported platform; on other systems every entry point fails with a
 * clear error instead of pretending to work.
 *
 * Design:
 *   - A Window is a refcounted handle (bloom_desktop_window). Native HWNDs
 *     never escape and are destroyed in the runtime.
 *   - Controls are integer IDs owned by their window (max 512).
 *   - Events are bounded per-window ring buffers. The pump dispatches
 *     Windows messages one at a time; a private wake message avoids busy
 *     polling and lost events after cross-thread SendMessage.
 *   - All public coordinates and sizes are logical units at 96 DPI.
 *   - Strings cross the Win32 boundary as UTF-8 <-> UTF-16 with validation.
 */

#ifdef _WIN32

#define BLOOM_DESKTOP_MAX_WINDOWS 64
#define BLOOM_DESKTOP_MAX_CONTROLS 512
#define BLOOM_DESKTOP_EVENT_CAP 64
#define BLOOM_DESKTOP_MAX_TEXT (1024 * 1024)
#define BLOOM_DESKTOP_WAKE (WM_APP + 1)
#define BLOOM_DESKTOP_MAGIC 0x424C444Bu /* "BLDK" */

typedef struct bloom_desktop_control {
    HWND hwnd;
    int64_t id;
    int kind; /* 0 label, 1 button, 2 checkbox, 3 edit */
} bloom_desktop_control;

typedef struct bloom_desktop_event {
    int kind; /* 0 click, 1 check, 2 input, 3 close */
    int64_t control;
    bloom_str text;
    bool checked;
} bloom_desktop_event;

typedef struct bloom_desktop_window {
    HWND hwnd;
    DWORD thread_id;
    uint32_t magic;
    int rc;
    bool closed;
    bool counted;
    int dpi;
    int64_t next_control_id;
    int control_count;
    bloom_desktop_control controls[BLOOM_DESKTOP_MAX_CONTROLS];
    int event_head;
    int event_count;
    int suppress_events;
    bloom_desktop_event events[BLOOM_DESKTOP_EVENT_CAP];
} bloom_desktop_window;

static bloom_desktop_window *bloom_desktop_windows[BLOOM_DESKTOP_MAX_WINDOWS];
static int bloom_desktop_window_count = 0;
static int bloom_desktop_native_window_count = 0;

static bloom_str bloom_desktop_msg(const char *op, const char *detail) {
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

static bloom_str bloom_desktop_werr(const char *op, DWORD code) {
    char buf[96];
    snprintf(buf, sizeof buf, "%s: Windows error %lu", op, (unsigned long)code);
    return bloom_str_from_bytes(buf, strlen(buf));
}

static bool bloom_desktop_has_ctl(bloom_str s) {
    if (s.len > BLOOM_DESKTOP_MAX_TEXT) {
        return true;
    }
    for (size_t i = 0; i < s.len; i++) {
        unsigned char c = (unsigned char)s.data[i];
        if (c < 0x20 || c == 0x7F) {
            return true;
        }
    }
    return false;
}

static bool bloom_desktop_text_ok(bloom_str s) {
    if (s.len > BLOOM_DESKTOP_MAX_TEXT || bloom_desktop_has_ctl(s)) {
        return false;
    }
    return s.len == 0 || memchr(s.data, 0, s.len) == NULL;
}

static wchar_t *bloom_desktop_to_utf16(bloom_str s) {
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
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, s.data, (int)s.len, w, need)
        != need) {
        free(w);
        return NULL;
    }
    w[need] = L'\0';
    return w;
}

static bloom_str bloom_desktop_from_utf16(const wchar_t *w, int wlen, bool *ok) {
    *ok = true;
    if (wlen <= 0) {
        return bloom_str_from_bytes("", 0);
    }
    int need = WideCharToMultiByte(CP_UTF8, 0, w, wlen, NULL, 0, NULL, NULL);
    if (need <= 0) {
        *ok = false;
        return bloom_str_from_bytes("", 0);
    }
    char *buf = (char *)malloc((size_t)need + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    if (WideCharToMultiByte(CP_UTF8, 0, w, wlen, buf, need, NULL, NULL) != need) {
        free(buf);
        *ok = false;
        return bloom_str_from_bytes("", 0);
    }
    bloom_str s = bloom_str_from_bytes(buf, (size_t)need);
    free(buf);
    return s;
}

static wchar_t *bloom_desktop_read_text(HWND hwnd, bool *ok) {
    *ok = false;
    int len = GetWindowTextLengthW(hwnd);
    if (len < 0) {
        return NULL;
    }
    wchar_t *buf = (wchar_t *)malloc(((size_t)len + 1) * sizeof(wchar_t));
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    int got = GetWindowTextW(hwnd, buf, len + 1);
    if (got < 0) {
        free(buf);
        return NULL;
    }
    buf[got] = L'\0';
    *ok = true;
    return buf;
}

/* ---- DPI ------------------------------------------------------------- */

typedef HANDLE(WINAPI *bloom_desktop_set_ctx_fn)(HANDLE);
typedef UINT(WINAPI *bloom_desktop_get_dpi_fn)(HWND);
typedef BOOL(WINAPI *bloom_desktop_adjust_fn)(LPRECT, DWORD, BOOL, DWORD, UINT);

static void bloom_desktop_enable_dpi(void) {
    static bool done = false;
    if (done) {
        return;
    }
    done = true;
    HMODULE user32 = GetModuleHandleW(L"user32.dll");
    if (user32 != NULL) {
        bloom_desktop_set_ctx_fn set_ctx = (bloom_desktop_set_ctx_fn)(void *)
            GetProcAddress(user32, "SetProcessDpiAwarenessContext");
        if (set_ctx != NULL && set_ctx((HANDLE)(intptr_t)-4)) {
            return;
        }
        if (set_ctx != NULL && set_ctx((HANDLE)(intptr_t)-3)) {
            return;
        }
    }
    SetProcessDPIAware();
}

static UINT bloom_desktop_system_dpi(void) {
    HDC dc = GetDC(NULL);
    if (dc == NULL) {
        return 96;
    }
    int dpi = GetDeviceCaps(dc, LOGPIXELSX);
    ReleaseDC(NULL, dc);
    return dpi > 0 ? (UINT)dpi : 96;
}

static UINT bloom_desktop_dpi_for(HWND hwnd) {
    static bloom_desktop_get_dpi_fn get_dpi = NULL;
    static bool tried = false;
    if (!tried) {
        tried = true;
        HMODULE user32 = GetModuleHandleW(L"user32.dll");
        if (user32 != NULL) {
            get_dpi = (bloom_desktop_get_dpi_fn)(void *)GetProcAddress(
                user32,
                "GetDpiForWindow"
            );
        }
    }
    if (get_dpi != NULL && IsWindow(hwnd)) {
        UINT dpi = get_dpi(hwnd);
        if (dpi >= 72 && dpi <= 960) {
            return dpi;
        }
    }
    return bloom_desktop_system_dpi();
}

/* ---- validation and registry ----------------------------------------- */

static bool bloom_desktop_is_live(bloom_desktop_window *w) {
    if (w == NULL || w->magic != BLOOM_DESKTOP_MAGIC) {
        return false;
    }
    for (int i = 0; i < bloom_desktop_window_count; i++) {
        if (bloom_desktop_windows[i] == w) {
            return true;
        }
    }
    return false;
}

static bloom_desktop_window *bloom_desktop_window_from(
    bloom_struct_std_desktop_Window handle
) {
    bloom_desktop_window *w = (bloom_desktop_window *)(intptr_t)handle.f_id;
    if (!bloom_desktop_is_live(w)) {
        return NULL;
    }
    return w;
}

static bool bloom_desktop_thread_ok(bloom_desktop_window *w) {
    return w->thread_id == GetCurrentThreadId();
}

static bloom_desktop_control *bloom_desktop_find_control(
    bloom_desktop_window *w,
    int64_t id
) {
    for (int i = 0; i < w->control_count; i++) {
        if (w->controls[i].id == id && IsWindow(w->controls[i].hwnd)) {
            return &w->controls[i];
        }
    }
    return NULL;
}

static bool bloom_desktop_register(bloom_desktop_window *w) {
    if (bloom_desktop_window_count >= BLOOM_DESKTOP_MAX_WINDOWS) {
        return false;
    }
    bloom_desktop_windows[bloom_desktop_window_count++] = w;
    return true;
}

static void bloom_desktop_unregister(bloom_desktop_window *w) {
    for (int i = 0; i < bloom_desktop_window_count; i++) {
        if (bloom_desktop_windows[i] == w) {
            for (int j = i; j + 1 < bloom_desktop_window_count; j++) {
                bloom_desktop_windows[j] = bloom_desktop_windows[j + 1];
            }
            bloom_desktop_window_count--;
            return;
        }
    }
}

/* ---- Result helpers --------------------------------------------------- */

static bloom_enum_Result_std_desktop_Window_str bloom_desktop_ok_window(
    bloom_desktop_window *w
) {
    bloom_enum_Result_std_desktop_Window_str r;
    r.tag = 0;
    r.data.m_Ok._0.f_id = (bloom_desktop_window_handle)(intptr_t)w;
    return r;
}

static bloom_enum_Result_std_desktop_Window_str bloom_desktop_err_window(
    bloom_str m
) {
    bloom_enum_Result_std_desktop_Window_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_bool_str bloom_desktop_ok_bool(void) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = true;
    return r;
}

static bloom_enum_Result_bool_str bloom_desktop_ok_bool_value(bool value) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = value;
    return r;
}

static bloom_enum_Result_bool_str bloom_desktop_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_i64_str bloom_desktop_ok_i64(int64_t v) {
    bloom_enum_Result_i64_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_i64_str bloom_desktop_err_i64(bloom_str m) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_str_str bloom_desktop_ok_str(bloom_str v) {
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_str_str bloom_desktop_err_str(bloom_str m) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

/* ---- events ----------------------------------------------------------- */

static const char *bloom_desktop_kind_name(int kind) {
    switch (kind) {
        case 0:
            return "click";
        case 1:
            return "check";
        case 2:
            return "input";
        default:
            return "close";
    }
}

static void bloom_desktop_push_event(
    bloom_desktop_window *w,
    int kind,
    int64_t control,
    bloom_str text,
    bool checked
) {
    if (w->event_count == BLOOM_DESKTOP_EVENT_CAP) {
        bloom_desktop_event *old = &w->events[w->event_head];
        bloom_str_drop(&old->text);
        old->text.data = NULL;
        old->text.len = 0;
        old->text.rc = 0;
        w->event_head = (w->event_head + 1) % BLOOM_DESKTOP_EVENT_CAP;
        w->event_count--;
    }
    int slot = (w->event_head + w->event_count) % BLOOM_DESKTOP_EVENT_CAP;
    w->events[slot].kind = kind;
    w->events[slot].control = control;
    w->events[slot].text = text;
    w->events[slot].checked = checked;
    w->event_count++;
    if (w->hwnd != NULL) {
        PostMessageW(w->hwnd, BLOOM_DESKTOP_WAKE, 0, 0);
    }
}

static bloom_desktop_event bloom_desktop_pop_event(bloom_desktop_window *w) {
    bloom_desktop_event ev = w->events[w->event_head];
    w->events[w->event_head].text.data = NULL;
    w->events[w->event_head].text.len = 0;
    w->events[w->event_head].text.rc = 0;
    w->event_head = (w->event_head + 1) % BLOOM_DESKTOP_EVENT_CAP;
    w->event_count--;
    return ev;
}

/* ---- window close ----------------------------------------------------- */

static bool bloom_desktop_close_impl(bloom_desktop_window *w) {
    if (w->closed) {
        return false;
    }
    w->closed = true;
    bloom_desktop_push_event(w, 3, 0, bloom_str_from_bytes("", 0), false);
    HWND hwnd = w->hwnd;
    w->hwnd = NULL;
    if (hwnd != NULL && IsWindow(hwnd)) {
        DestroyWindow(hwnd);
    }
    return true;
}

/* ---- window class and procedure -------------------------------------- */

static LRESULT CALLBACK bloom_desktop_proc(
    HWND hwnd,
    UINT msg,
    WPARAM wp,
    LPARAM lp
) {
    bloom_desktop_window *w = (bloom_desktop_window *)(void *)GetWindowLongPtrW(
        hwnd,
        GWLP_USERDATA
    );
    if (msg == WM_NCCREATE) {
        CREATESTRUCTW *cs = (CREATESTRUCTW *)lp;
        w = (bloom_desktop_window *)cs->lpCreateParams;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (LONG_PTR)(void *)w);
    }
    if (w == NULL) {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    switch (msg) {
        case WM_COMMAND: {
            int64_t id = (int64_t)LOWORD(wp);
            int code = HIWORD(wp);
            bloom_desktop_control *c = bloom_desktop_find_control(w, id);
            if (c == NULL) {
                break;
            }
            if (c->kind == 1 && code == BN_CLICKED) {
                bloom_desktop_push_event(w, 0, id, bloom_str_from_bytes("", 0), false);
            } else if (c->kind == 2 && code == BN_CLICKED) {
                LRESULT state = SendMessageW(c->hwnd, BM_GETCHECK, 0, 0);
                bloom_desktop_push_event(
                    w,
                    1,
                    id,
                    bloom_str_from_bytes("", 0),
                    state == BST_CHECKED
                );
            } else if (c->kind == 3 && code == EN_CHANGE && w->suppress_events == 0) {
                bool ok = false;
                wchar_t *text = bloom_desktop_read_text(c->hwnd, &ok);
                if (ok) {
                    bloom_str s = bloom_desktop_from_utf16(
                        text,
                        (int)wcslen(text),
                        &ok
                    );
                    bloom_desktop_push_event(w, 2, id, s, false);
                }
                free(text);
            }
            break;
        }
        case WM_CLOSE:
            bloom_desktop_close_impl(w);
            return 0;
        case WM_DESTROY:
            if (w->counted) {
                w->counted = false;
                if (bloom_desktop_native_window_count > 0) {
                    bloom_desktop_native_window_count--;
                }
            }
            return 0;
        case WM_DPICHANGED: {
            w->dpi = (int)HIWORD(wp);
            RECT *suggested = (RECT *)lp;
            if (suggested != NULL) {
                SetWindowPos(
                    hwnd,
                    NULL,
                    suggested->left,
                    suggested->top,
                    suggested->right - suggested->left,
                    suggested->bottom - suggested->top,
                    SWP_NOZORDER | SWP_NOACTIVATE
                );
            }
            return 0;
        }
        case BLOOM_DESKTOP_WAKE:
            return 0;
        default:
            break;
    }
    return DefWindowProcW(hwnd, msg, wp, lp);
}

static bool bloom_desktop_class_ready = false;

static bool bloom_desktop_ensure_class(void) {
    if (bloom_desktop_class_ready) {
        return true;
    }
    WNDCLASSEXW wc;
    memset(&wc, 0, sizeof wc);
    wc.cbSize = sizeof wc;
    wc.lpfnWndProc = bloom_desktop_proc;
    wc.hInstance = GetModuleHandleW(NULL);
    wc.hCursor = LoadCursorW(NULL, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
    wc.lpszClassName = L"BloomDesktopWindow";
    if (RegisterClassExW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) {
        return false;
    }
    bloom_desktop_class_ready = true;
    return true;
}

static bool bloom_desktop_set_client_size(
    bloom_desktop_window *w,
    int64_t width,
    int64_t height
) {
    if (!IsWindow(w->hwnd)) {
        return false;
    }
    UINT dpi = bloom_desktop_dpi_for(w->hwnd);
    int px = (int)((width * dpi + 48) / 96);
    int py = (int)((height * dpi + 48) / 96);
    if (px < 1) {
        px = 1;
    }
    if (py < 1) {
        py = 1;
    }
    RECT rect;
    rect.left = 0;
    rect.top = 0;
    rect.right = px;
    rect.bottom = py;
    DWORD style = (DWORD)GetWindowLongPtrW(w->hwnd, GWL_STYLE);
    DWORD exstyle = (DWORD)GetWindowLongPtrW(w->hwnd, GWL_EXSTYLE);
    static bloom_desktop_adjust_fn adjust = NULL;
    static bool tried = false;
    if (!tried) {
        tried = true;
        HMODULE user32 = GetModuleHandleW(L"user32.dll");
        if (user32 != NULL) {
            adjust = (bloom_desktop_adjust_fn)(void *)GetProcAddress(
                user32,
                "AdjustWindowRectExForDpi"
            );
        }
    }
    BOOL ok = FALSE;
    if (adjust != NULL) {
        ok = adjust(&rect, style, FALSE, exstyle, dpi);
    } else {
        ok = AdjustWindowRectEx(&rect, style, FALSE, exstyle);
    }
    if (!ok) {
        return false;
    }
    int total_w = rect.right - rect.left;
    int total_h = rect.bottom - rect.top;
    return SetWindowPos(
        w->hwnd,
        NULL,
        0,
        0,
        total_w,
        total_h,
        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
    ) != 0;
}

/* ---- public: window --------------------------------------------------- */

bool bloom_desktop_available(void) {
    return true;
}

bloom_enum_Result_std_desktop_Window_str bloom_desktop_window_create(
    bloom_str title,
    int64_t width,
    int64_t height
) {
    if (!bloom_desktop_text_ok(title)) {
        return bloom_desktop_err_window(bloom_desktop_msg(
            "window_create",
            "title exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    if (width < 1 || width > 16384 || height < 1 || height > 16384) {
        return bloom_desktop_err_window(bloom_desktop_msg(
            "window_create",
            "width and height must be between 1 and 16384 logical units"
        ));
    }
    if (bloom_desktop_native_window_count >= BLOOM_DESKTOP_MAX_WINDOWS) {
        return bloom_desktop_err_window(bloom_desktop_msg(
            "window_create",
            "too many windows (max 64)"
        ));
    }
    bloom_desktop_enable_dpi();
    if (!bloom_desktop_ensure_class()) {
        return bloom_desktop_err_window(bloom_desktop_werr(
            "window_create",
            GetLastError()
        ));
    }
    wchar_t *wt = bloom_desktop_to_utf16(title);
    if (wt == NULL) {
        return bloom_desktop_err_window(bloom_desktop_msg(
            "window_create",
            "title is not valid UTF-8"
        ));
    }
    bloom_desktop_window *w = (bloom_desktop_window *)calloc(1, sizeof *w);
    if (w == NULL) {
        free(wt);
        bloom_panicf(NULL, 0, "out of memory");
    }
    w->magic = BLOOM_DESKTOP_MAGIC;
    w->thread_id = GetCurrentThreadId();
    w->rc = 1;
    bloom_live_allocs++;
    w->dpi = (int)bloom_desktop_system_dpi();
    w->next_control_id = 100;
    w->event_head = 0;
    w->event_count = 0;
    UINT dpi = (UINT)w->dpi;
    int px = (int)((width * dpi + 48) / 96);
    int py = (int)((height * dpi + 48) / 96);
    w->hwnd = CreateWindowExW(
        0,
        L"BloomDesktopWindow",
        wt,
        WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        px < 1 ? 1 : px,
        py < 1 ? 1 : py,
        NULL,
        NULL,
        GetModuleHandleW(NULL),
        w
    );
    free(wt);
    if (w->hwnd == NULL) {
        DWORD err = GetLastError();
        w->magic = 0;
        free(w);
        return bloom_desktop_err_window(bloom_desktop_werr("window_create", err));
    }
    w->dpi = (int)bloom_desktop_dpi_for(w->hwnd);
    if (!bloom_desktop_register(w)) {
        DestroyWindow(w->hwnd);
        w->magic = 0;
        free(w);
        return bloom_desktop_err_window(bloom_desktop_msg(
            "window_create",
            "too many windows (max 64)"
        ));
    }
    w->counted = true;
    bloom_desktop_native_window_count++;
    if (!bloom_desktop_set_client_size(w, width, height)) {
        DWORD err = GetLastError();
        bloom_desktop_close_impl(w);
        for (int i = 0; i < w->event_count; i++) {
            int slot = (w->event_head + i) % BLOOM_DESKTOP_EVENT_CAP;
            bloom_str_drop(&w->events[slot].text);
        }
        bloom_desktop_unregister(w);
        w->magic = 0;
        free(w);
        bloom_live_allocs--;
        return bloom_desktop_err_window(bloom_desktop_werr("window_create", err));
    }
    return bloom_desktop_ok_window(w);
}

static bool bloom_desktop_prepare(
    bloom_struct_std_desktop_Window handle,
    const char *op,
    bloom_str *err
) {
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    if (w == NULL) {
        *err = bloom_desktop_msg(op, "invalid window handle");
        return false;
    }
    if (!bloom_desktop_thread_ok(w)) {
        *err = bloom_desktop_msg(op, "desktop calls must run on the window's thread");
        return false;
    }
    if (w->closed || w->hwnd == NULL) {
        *err = bloom_desktop_msg(op, "window is closed");
        return false;
    }
    return true;
}

bloom_enum_Result_bool_str bloom_desktop_window_show(
    bloom_struct_std_desktop_Window handle
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(handle, "window_show", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    ShowWindow(w->hwnd, SW_SHOW);
    UpdateWindow(w->hwnd);
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_window_hide(
    bloom_struct_std_desktop_Window handle
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(handle, "window_hide", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    ShowWindow(w->hwnd, SW_HIDE);
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_window_set_title(
    bloom_struct_std_desktop_Window handle,
    bloom_str title
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(handle, "window_set_title", &err)) {
        return bloom_desktop_err_bool(err);
    }
    if (!bloom_desktop_text_ok(title)) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "window_set_title",
            "title exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    wchar_t *wt = bloom_desktop_to_utf16(title);
    if (wt == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "window_set_title",
            "title is not valid UTF-8"
        ));
    }
    BOOL ok = SetWindowTextW(w->hwnd, wt);
    free(wt);
    if (!ok) {
        return bloom_desktop_err_bool(bloom_desktop_werr(
            "window_set_title",
            GetLastError()
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_window_set_size(
    bloom_struct_std_desktop_Window handle,
    int64_t width,
    int64_t height
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(handle, "window_set_size", &err)) {
        return bloom_desktop_err_bool(err);
    }
    if (width < 1 || width > 16384 || height < 1 || height > 16384) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "window_set_size",
            "width and height must be between 1 and 16384 logical units"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    if (!bloom_desktop_set_client_size(w, width, height)) {
        return bloom_desktop_err_bool(bloom_desktop_werr(
            "window_set_size",
            GetLastError()
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_window_set_position(
    bloom_struct_std_desktop_Window handle,
    int64_t x,
    int64_t y
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(handle, "window_set_position", &err)) {
        return bloom_desktop_err_bool(err);
    }
    if (x < -32768 || x > 32768 || y < -32768 || y > 32768) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "window_set_position",
            "x and y must be between -32768 and 32768 logical units"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    int px = (int)((x * w->dpi + 48) / 96);
    int py = (int)((y * w->dpi + 48) / 96);
    if (!SetWindowPos(
            w->hwnd,
            NULL,
            px,
            py,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE
        )) {
        return bloom_desktop_err_bool(bloom_desktop_werr(
            "window_set_position",
            GetLastError()
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_window_close(
    bloom_struct_std_desktop_Window handle
) {
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    if (w == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "window_close",
            "invalid window handle"
        ));
    }
    if (!bloom_desktop_thread_ok(w)) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "window_close",
            "desktop calls must run on the window's thread"
        ));
    }
    return bloom_desktop_ok_bool_value(bloom_desktop_close_impl(w));
}

bool bloom_desktop_window_alive(bloom_struct_std_desktop_Window handle) {
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    if (w == NULL || w->closed) {
        return false;
    }
    return IsWindow(w->hwnd) != 0;
}

bloom_enum_Result_i64_str bloom_desktop_window_dpi(
    bloom_struct_std_desktop_Window handle
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(handle, "window_dpi", &err)) {
        return bloom_desktop_err_i64(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    w->dpi = (int)bloom_desktop_dpi_for(w->hwnd);
    return bloom_desktop_ok_i64(w->dpi);
}

bloom_enum_Result_std_desktop_Size_str bloom_desktop_window_client_size(
    bloom_struct_std_desktop_Window handle
) {
    bloom_str err = {0, NULL, 0};
    bloom_enum_Result_std_desktop_Size_str r;
    if (!bloom_desktop_prepare(handle, "window_client_size", &err)) {
        r.tag = 1;
        r.data.m_Err._0 = err;
        return r;
    }
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    RECT rect;
    if (!GetClientRect(w->hwnd, &rect)) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_desktop_werr("window_client_size", GetLastError());
        return r;
    }
    UINT dpi = bloom_desktop_dpi_for(w->hwnd);
    int pw = rect.right - rect.left;
    int ph = rect.bottom - rect.top;
    r.tag = 0;
    r.data.m_Ok._0.f_width = (pw * 96 + (int)dpi / 2) / (int)dpi;
    r.data.m_Ok._0.f_height = (ph * 96 + (int)dpi / 2) / (int)dpi;
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_message_box(
    bloom_str title,
    bloom_str message
) {
    if (!bloom_desktop_text_ok(title) || !bloom_desktop_text_ok(message)) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "message_box",
            "title/message exceed 1048576 bytes or contain control/NUL bytes"
        ));
    }
    wchar_t *wt = bloom_desktop_to_utf16(title);
    wchar_t *wm = bloom_desktop_to_utf16(message);
    if (wt == NULL || wm == NULL) {
        free(wt);
        free(wm);
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "message_box",
            "title/message are not valid UTF-8"
        ));
    }
    int result = MessageBoxW(NULL, wm, wt, MB_OK | MB_ICONINFORMATION);
    free(wt);
    free(wm);
    return bloom_desktop_ok_bool_value(result == IDOK);
}

/* ---- public: controls ------------------------------------------------- */

static bloom_enum_Result_i64_str bloom_desktop_create_control(
    bloom_struct_std_desktop_Window handle,
    bloom_str text,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height,
    const wchar_t *class_name,
    DWORD style,
    int kind,
    const char *op
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(handle, op, &err)) {
        return bloom_desktop_err_i64(err);
    }
    if (!bloom_desktop_text_ok(text)) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            op,
            "text exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    if (x < -32768 || x > 32768 || y < -32768 || y > 32768) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            op,
            "x and y must be between -32768 and 32768 logical units"
        ));
    }
    if (width < 1 || width > 16384 || height < 1 || height > 16384) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            op,
            "width and height must be between 1 and 16384 logical units"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    if (w->control_count >= BLOOM_DESKTOP_MAX_CONTROLS) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            op,
            "too many controls in window (max 512)"
        ));
    }
    wchar_t *wt = bloom_desktop_to_utf16(text);
    if (wt == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(op, "text is not valid UTF-8"));
    }
    int64_t id = w->next_control_id++;
    int px = (int)((x * w->dpi + 48) / 96);
    int py = (int)((y * w->dpi + 48) / 96);
    int pw = (int)((width * w->dpi + 48) / 96);
    int ph = (int)((height * w->dpi + 48) / 96);
    HWND hwnd = CreateWindowExW(
        0,
        class_name,
        wt,
        WS_CHILD | WS_VISIBLE | style,
        px,
        py,
        pw < 1 ? 1 : pw,
        ph < 1 ? 1 : ph,
        w->hwnd,
        (HMENU)(intptr_t)id,
        GetModuleHandleW(NULL),
        NULL
    );
    free(wt);
    if (hwnd == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_werr(op, GetLastError()));
    }
    SendMessageW(
        hwnd,
        WM_SETFONT,
        (WPARAM)GetStockObject(DEFAULT_GUI_FONT),
        TRUE
    );
    bloom_desktop_control *c = &w->controls[w->control_count++];
    c->hwnd = hwnd;
    c->id = id;
    c->kind = kind;
    return bloom_desktop_ok_i64(id);
}

bloom_enum_Result_i64_str bloom_desktop_label_create(
    bloom_struct_std_desktop_Window window,
    bloom_str text,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    return bloom_desktop_create_control(
        window,
        text,
        x,
        y,
        width,
        height,
        L"STATIC",
        SS_LEFT,
        0,
        "label_create"
    );
}

bloom_enum_Result_i64_str bloom_desktop_button_create(
    bloom_struct_std_desktop_Window window,
    bloom_str text,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    return bloom_desktop_create_control(
        window,
        text,
        x,
        y,
        width,
        height,
        L"BUTTON",
        BS_PUSHBUTTON | WS_TABSTOP,
        1,
        "button_create"
    );
}

bloom_enum_Result_i64_str bloom_desktop_text_input_create(
    bloom_struct_std_desktop_Window window,
    bloom_str text,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    return bloom_desktop_create_control(
        window,
        text,
        x,
        y,
        width,
        height,
        L"EDIT",
        ES_LEFT | ES_AUTOHSCROLL | WS_TABSTOP,
        3,
        "text_input_create"
    );
}

bloom_enum_Result_i64_str bloom_desktop_checkbox_create(
    bloom_struct_std_desktop_Window window,
    bloom_str text,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    return bloom_desktop_create_control(
        window,
        text,
        x,
        y,
        width,
        height,
        L"BUTTON",
        BS_AUTOCHECKBOX | WS_TABSTOP,
        2,
        "checkbox_create"
    );
}

static bloom_desktop_control *bloom_desktop_control_for(
    bloom_struct_std_desktop_Window handle,
    int64_t control,
    int want_kind,
    const char *op,
    bloom_str *err
) {
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    if (w == NULL) {
        *err = bloom_desktop_msg(op, "invalid window handle");
        return NULL;
    }
    if (!bloom_desktop_thread_ok(w)) {
        *err = bloom_desktop_msg(op, "desktop calls must run on the window's thread");
        return NULL;
    }
    if (w->closed || w->hwnd == NULL) {
        *err = bloom_desktop_msg(op, "window is closed");
        return NULL;
    }
    bloom_desktop_control *c = bloom_desktop_find_control(w, control);
    if (c == NULL) {
        *err = bloom_desktop_msg(op, "unknown control id");
        return NULL;
    }
    if (want_kind >= 0 && c->kind != want_kind) {
        *err = bloom_desktop_msg(op, "control has the wrong type for this operation");
        return NULL;
    }
    return c;
}

bloom_enum_Result_str_str bloom_desktop_control_text(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        -1,
        "control_text",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_str(err);
    }
    bool ok = false;
    wchar_t *text = bloom_desktop_read_text(c->hwnd, &ok);
    if (!ok) {
        free(text);
        return bloom_desktop_err_str(bloom_desktop_werr(
            "control_text",
            GetLastError()
        ));
    }
    bloom_str s = bloom_desktop_from_utf16(text, (int)wcslen(text), &ok);
    free(text);
    if (!ok) {
        return bloom_desktop_err_str(bloom_desktop_msg(
            "control_text",
            "control text is not valid Unicode"
        ));
    }
    return bloom_desktop_ok_str(s);
}

bloom_enum_Result_bool_str bloom_desktop_control_set_text(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    bloom_str text
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        -1,
        "control_set_text",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    if (!bloom_desktop_text_ok(text)) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "control_set_text",
            "text exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    wchar_t *wt = bloom_desktop_to_utf16(text);
    if (wt == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "control_set_text",
            "text is not valid UTF-8"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    w->suppress_events++;
    BOOL ok = SetWindowTextW(c->hwnd, wt);
    w->suppress_events--;
    free(wt);
    if (!ok) {
        return bloom_desktop_err_bool(bloom_desktop_werr(
            "control_set_text",
            GetLastError()
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_control_checked(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        2,
        "control_checked",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    LRESULT state = SendMessageW(c->hwnd, BM_GETCHECK, 0, 0);
    return bloom_desktop_ok_bool_value(state == BST_CHECKED);
}

bloom_enum_Result_bool_str bloom_desktop_control_set_checked(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    bool checked
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        2,
        "control_set_checked",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    SendMessageW(
        c->hwnd,
        BM_SETCHECK,
        checked ? BST_CHECKED : BST_UNCHECKED,
        0
    );
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_control_enabled(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        -1,
        "control_enabled",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    return bloom_desktop_ok_bool_value(IsWindowEnabled(c->hwnd) != 0);
}

bloom_enum_Result_bool_str bloom_desktop_control_set_enabled(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    bool enabled
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        -1,
        "control_set_enabled",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    EnableWindow(c->hwnd, enabled ? TRUE : FALSE);
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_control_visible(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        -1,
        "control_visible",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    return bloom_desktop_ok_bool_value(IsWindowVisible(c->hwnd) != 0);
}

bloom_enum_Result_bool_str bloom_desktop_control_set_visible(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    bool visible
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        -1,
        "control_set_visible",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    ShowWindow(c->hwnd, visible ? SW_SHOW : SW_HIDE);
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_std_desktop_Bounds_str bloom_desktop_control_bounds(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_enum_Result_std_desktop_Bounds_str r;
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        -1,
        "control_bounds",
        &err
    );
    if (c == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = err;
        return r;
    }
    RECT rect;
    if (!GetWindowRect(c->hwnd, &rect)) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_desktop_werr("control_bounds", GetLastError());
        return r;
    }
    POINT top;
    top.x = rect.left;
    top.y = rect.top;
    if (!ScreenToClient(GetParent(c->hwnd), &top)) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_desktop_werr("control_bounds", GetLastError());
        return r;
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    int dpi = w->dpi > 0 ? w->dpi : 96;
    r.tag = 0;
    r.data.m_Ok._0.f_x = (top.x * 96 + dpi / 2) / dpi;
    r.data.m_Ok._0.f_y = (top.y * 96 + dpi / 2) / dpi;
    r.data.m_Ok._0.f_width = ((rect.right - rect.left) * 96 + dpi / 2) / dpi;
    r.data.m_Ok._0.f_height = ((rect.bottom - rect.top) * 96 + dpi / 2) / dpi;
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_control_set_bounds(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        -1,
        "control_set_bounds",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    if (x < -32768 || x > 32768 || y < -32768 || y > 32768) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "control_set_bounds",
            "x and y must be between -32768 and 32768 logical units"
        ));
    }
    if (width < 1 || width > 16384 || height < 1 || height > 16384) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "control_set_bounds",
            "width and height must be between 1 and 16384 logical units"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    int px = (int)((x * w->dpi + 48) / 96);
    int py = (int)((y * w->dpi + 48) / 96);
    int pw = (int)((width * w->dpi + 48) / 96);
    int ph = (int)((height * w->dpi + 48) / 96);
    if (!SetWindowPos(
            c->hwnd,
            NULL,
            px,
            py,
            pw < 1 ? 1 : pw,
            ph < 1 ? 1 : ph,
            SWP_NOZORDER | SWP_NOACTIVATE
        )) {
        return bloom_desktop_err_bool(bloom_desktop_werr(
            "control_set_bounds",
            GetLastError()
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_control_focus(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        -1,
        "control_focus",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    SetFocus(c->hwnd);
    return bloom_desktop_ok_bool();
}

/* ---- public: events --------------------------------------------------- */

bloom_enum_Result_std_desktop_Event_str bloom_desktop_next_event(
    bloom_struct_std_desktop_Window handle
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_window *w = bloom_desktop_window_from(handle);
    bloom_enum_Result_std_desktop_Event_str r;
    if (w == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_desktop_msg("next_event", "invalid window handle");
        return r;
    }
    if (!bloom_desktop_thread_ok(w)) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_desktop_msg(
            "next_event",
            "desktop calls must run on the window's thread"
        );
        return r;
    }
    while (w->event_count == 0) {
        if (w->closed) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_desktop_msg("next_event", "window is closed");
            return r;
        }
        MSG msg;
        BOOL got = GetMessageW(&msg, NULL, 0, 0);
        if (got == -1) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_desktop_werr("next_event", GetLastError());
            return r;
        }
        if (got == 0) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_desktop_msg(
                "next_event",
                "message loop ended while the window was open"
            );
            return r;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
        if (w->event_count == 0 && w->closed) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_desktop_msg("next_event", "window is closed");
            return r;
        }
        if (w->event_count == 0 && msg.message == WM_QUIT) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_desktop_msg("next_event", "message loop ended");
            return r;
        }
    }
    bloom_desktop_event ev = bloom_desktop_pop_event(w);
    r.tag = 0;
    r.data.m_Ok._0.f_kind = bloom_str_from_bytes(
        bloom_desktop_kind_name(ev.kind),
        strlen(bloom_desktop_kind_name(ev.kind))
    );
    r.data.m_Ok._0.f_control = ev.control;
    r.data.m_Ok._0.f_text = ev.text;
    r.data.m_Ok._0.f_checked = ev.checked;
    return r;
}

/* ---- handle lifetime -------------------------------------------------- */

bloom_desktop_window_handle bloom_desktop_window_retain(
    bloom_desktop_window_handle h
) {
    bloom_desktop_window *w = (bloom_desktop_window *)(intptr_t)h;
    if (bloom_desktop_is_live(w)) {
        w->rc++;
    }
    return h;
}

void bloom_desktop_window_release(bloom_desktop_window_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_desktop_window *w = (bloom_desktop_window *)(intptr_t)*h;
    *h = 0;
    if (!bloom_desktop_is_live(w)) {
        return;
    }
    if (w->rc > 0) {
        w->rc--;
    }
    if (w->rc != 0) {
        return;
    }
    if (!w->closed) {
        bloom_desktop_close_impl(w);
    }
    for (int i = 0; i < w->event_count; i++) {
        int slot = (w->event_head + i) % BLOOM_DESKTOP_EVENT_CAP;
        bloom_str_drop(&w->events[slot].text);
    }
    bloom_desktop_unregister(w);
    w->magic = 0;
    free(w);
    bloom_live_allocs--;
}

#else /* !_WIN32 */

bool bloom_desktop_available(void) {
    return false;
}

static bloom_str bloom_desktop_unsupported(const char *op) {
    char buf[128];
    snprintf(buf, sizeof buf, "%s: std::desktop is only supported on Windows", op);
    return bloom_str_from_bytes(buf, strlen(buf));
}

bloom_enum_Result_std_desktop_Window_str bloom_desktop_window_create(
    bloom_str title,
    int64_t width,
    int64_t height
) {
    bloom_enum_Result_std_desktop_Window_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("window_create");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_window_show(
    bloom_struct_std_desktop_Window handle
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("window_show");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_window_hide(
    bloom_struct_std_desktop_Window handle
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("window_hide");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_window_set_title(
    bloom_struct_std_desktop_Window handle,
    bloom_str title
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("window_set_title");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_window_set_size(
    bloom_struct_std_desktop_Window handle,
    int64_t width,
    int64_t height
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("window_set_size");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_window_set_position(
    bloom_struct_std_desktop_Window handle,
    int64_t x,
    int64_t y
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("window_set_position");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_window_close(
    bloom_struct_std_desktop_Window handle
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("window_close");
    return r;
}

bool bloom_desktop_window_alive(bloom_struct_std_desktop_Window handle) {
    return false;
}

bloom_enum_Result_i64_str bloom_desktop_window_dpi(
    bloom_struct_std_desktop_Window handle
) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("window_dpi");
    return r;
}

bloom_enum_Result_std_desktop_Size_str bloom_desktop_window_client_size(
    bloom_struct_std_desktop_Window handle
) {
    bloom_enum_Result_std_desktop_Size_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("window_client_size");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_message_box(
    bloom_str title,
    bloom_str message
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("message_box");
    return r;
}

bloom_enum_Result_i64_str bloom_desktop_label_create(
    bloom_struct_std_desktop_Window window,
    bloom_str text,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("label_create");
    return r;
}

bloom_enum_Result_i64_str bloom_desktop_button_create(
    bloom_struct_std_desktop_Window window,
    bloom_str text,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("button_create");
    return r;
}

bloom_enum_Result_i64_str bloom_desktop_text_input_create(
    bloom_struct_std_desktop_Window window,
    bloom_str text,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("text_input_create");
    return r;
}

bloom_enum_Result_i64_str bloom_desktop_checkbox_create(
    bloom_struct_std_desktop_Window window,
    bloom_str text,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("checkbox_create");
    return r;
}

bloom_enum_Result_str_str bloom_desktop_control_text(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_text");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_control_set_text(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    bloom_str text
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_set_text");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_control_checked(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_checked");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_control_set_checked(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    bool checked
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_set_checked");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_control_enabled(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_enabled");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_control_set_enabled(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    bool enabled
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_set_enabled");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_control_visible(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_visible");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_control_set_visible(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    bool visible
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_set_visible");
    return r;
}

bloom_enum_Result_std_desktop_Bounds_str bloom_desktop_control_bounds(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_enum_Result_std_desktop_Bounds_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_bounds");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_control_set_bounds(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_set_bounds");
    return r;
}

bloom_enum_Result_bool_str bloom_desktop_control_focus(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("control_focus");
    return r;
}

bloom_enum_Result_std_desktop_Event_str bloom_desktop_next_event(
    bloom_struct_std_desktop_Window handle
) {
    bloom_enum_Result_std_desktop_Event_str r;
    r.tag = 1;
    r.data.m_Err._0 = bloom_desktop_unsupported("next_event");
    return r;
}

bloom_desktop_window_handle bloom_desktop_window_retain(
    bloom_desktop_window_handle h
) {
    return h;
}

void bloom_desktop_window_release(bloom_desktop_window_handle *h) {
    if (h != NULL) {
        *h = 0;
    }
}

#endif /* _WIN32 */
