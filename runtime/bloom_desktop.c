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

#include <commdlg.h>

#define BLOOM_DESKTOP_MAX_WINDOWS 64
#define BLOOM_DESKTOP_MAX_CONTROLS 512
#define BLOOM_DESKTOP_EVENT_CAP 64
#define BLOOM_DESKTOP_MAX_TEXT (1024 * 1024)
#define BLOOM_DESKTOP_MAX_AREA_TEXT (64 * 1024)
#define BLOOM_DESKTOP_MAX_LAYOUTS 64
#define BLOOM_DESKTOP_MAX_LAYOUT_CHILDREN 256
#define BLOOM_DESKTOP_MAX_LAYOUT_DEPTH 32
#define BLOOM_DESKTOP_MAX_LAYOUT_NODES 2048
#define BLOOM_DESKTOP_MAX_MENU_ITEMS 128
#define BLOOM_DESKTOP_MENU_ID_BASE 1000
#define BLOOM_DESKTOP_LAYOUT_ID_BASE 10000
#define BLOOM_DESKTOP_WAKE (WM_APP + 1)
#define BLOOM_DESKTOP_MAGIC 0x424C444Bu /* "BLDK" */

typedef struct bloom_desktop_control {
    HWND hwnd;
    int64_t id;
    int kind; /* 0 label, 1 button, 2 checkbox, 3 edit, 4 combo, 5 textarea, 6 list */
} bloom_desktop_control;

typedef struct bloom_desktop_event {
    int kind; /* 0 click, 1 check, 2 input, 3 close, 4 select, 5 menu */
    int64_t control;
    bloom_str text;
    bool checked;
} bloom_desktop_event;

typedef struct bloom_desktop_menu_entry {
    int64_t id;
    int64_t parent_id;
    int position;
    bool submenu;
    bloom_str label;
} bloom_desktop_menu_entry;

typedef struct bloom_desktop_layout_child {
    int type; /* 0 control, 1 layout */
    int64_t control;
    int64_t layout_id;
    int64_t width;
    int64_t height;
    int flex;
    int align; /* -1 inherit, 0 start, 1 center, 2 end, 3 stretch */
} bloom_desktop_layout_child;

typedef struct bloom_desktop_layout {
    int64_t id;
    int kind; /* 0 row, 1 column */
    int64_t padding;
    int64_t spacing;
    int align;
    int64_t parent_id;
    int child_count;
    int child_cap;
    bloom_desktop_layout_child *children;
} bloom_desktop_layout;

typedef struct bloom_desktop_window {
    HWND hwnd;
    DWORD thread_id;
    uint32_t magic;
    int rc;
    bool closed;
    bool counted;
    bool close_delivered;
    bool layout_dirty;
    int dpi;
    int dropped_events;
    int64_t next_control_id;
    int control_count;
    bloom_desktop_control controls[BLOOM_DESKTOP_MAX_CONTROLS];
    int event_head;
    int event_count;
    int suppress_events;
    bloom_desktop_event events[BLOOM_DESKTOP_EVENT_CAP];
    HMENU menu;
    int64_t next_menu_id;
    int menu_count;
    bloom_desktop_menu_entry menu_entries[BLOOM_DESKTOP_MAX_MENU_ITEMS];
    int64_t next_layout_id;
    int layout_count;
    int64_t root_layout;
    bloom_desktop_layout layouts[BLOOM_DESKTOP_MAX_LAYOUTS];
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
        case 4:
            return "select";
        case 5:
            return "menu";
        default:
            return "close";
    }
}

static bloom_desktop_menu_entry *bloom_desktop_menu_entry_for(
    bloom_desktop_window *w,
    int64_t id
);
static bool bloom_desktop_layout_apply(bloom_desktop_window *w);
static bloom_str bloom_desktop_get_control_text(
    bloom_desktop_window *w,
    bloom_desktop_control *c
);
static bool bloom_desktop_set_control_text(
    bloom_desktop_window *w,
    bloom_desktop_control *c,
    bloom_str text
);

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
        w->dropped_events++;
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
    for (int i = 0; i < w->menu_count; i++) {
        bloom_str_drop(&w->menu_entries[i].label);
        w->menu_entries[i].label.data = NULL;
        w->menu_entries[i].label.len = 0;
        w->menu_entries[i].label.rc = 0;
    }
    w->menu_count = 0;
    for (int i = 0; i < w->layout_count; i++) {
        free(w->layouts[i].children);
        w->layouts[i].children = NULL;
        w->layouts[i].child_count = 0;
        w->layouts[i].child_cap = 0;
    }
    w->layout_count = 0;
    w->root_layout = 0;
    w->layout_dirty = false;
    HWND hwnd = w->hwnd;
    w->hwnd = NULL;
    if (w->menu != NULL) {
        if (hwnd != NULL && IsWindow(hwnd)) {
            SetMenu(hwnd, NULL);
        }
        DestroyMenu(w->menu);
        w->menu = NULL;
    }
    if (hwnd != NULL && IsWindow(hwnd)) {
        PostMessageW(hwnd, BLOOM_DESKTOP_WAKE, 0, 0);
        DestroyWindow(hwnd);
    }
    return true;
}

/* ---- window class and procedure -------------------------------------- */

static bloom_str bloom_desktop_selected_text(HWND hwnd, int kind, bool *ok) {
    *ok = true;
    int index = (int)SendMessageW(hwnd, kind == 4 ? CB_GETCURSEL : LB_GETCURSEL, 0, 0);
    if (index < 0) {
        return bloom_str_from_bytes("", 0);
    }
    int len = (int)SendMessageW(
        hwnd,
        kind == 4 ? CB_GETLBTEXTLEN : LB_GETTEXTLEN,
        (WPARAM)index,
        0
    );
    if (len < 0 || len > BLOOM_DESKTOP_MAX_TEXT) {
        *ok = false;
        return bloom_str_from_bytes("", 0);
    }
    wchar_t *buf = (wchar_t *)malloc(((size_t)len + 1) * sizeof(wchar_t));
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    LRESULT got = SendMessageW(
        hwnd,
        kind == 4 ? CB_GETLBTEXT : LB_GETTEXT,
        (WPARAM)index,
        (LPARAM)buf
    );
    bloom_str s = {0, NULL, 0};
    if (got >= 0 && got <= len) {
        s = bloom_desktop_from_utf16(buf, (int)got, ok);
    } else {
        *ok = false;
        s = bloom_str_from_bytes("", 0);
    }
    free(buf);
    return s;
}

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
            if (lp == 0) {
                bloom_desktop_menu_entry *entry = bloom_desktop_menu_entry_for(w, id);
                if (entry != NULL && !entry->submenu) {
                    bloom_desktop_push_event(
                        w,
                        5,
                        id,
                        bloom_str_clone(entry->label),
                        false
                    );
                }
                break;
            }
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
            } else if ((c->kind == 3 || c->kind == 5)
                && code == EN_CHANGE
                && w->suppress_events == 0) {
                bloom_str s = bloom_desktop_get_control_text(w, c);
                bloom_desktop_push_event(w, 2, id, s, false);
            } else if ((c->kind == 4 || c->kind == 6) && code == (c->kind == 4 ? CBN_SELCHANGE : LBN_SELCHANGE)) {
                bool ok = false;
                bloom_str s = bloom_desktop_selected_text(c->hwnd, c->kind, &ok);
                if (ok) {
                    bloom_desktop_push_event(w, 4, id, s, false);
                } else {
                    bloom_str_drop(&s);
                }
            }
            break;
        }
        case WM_CLOSE:
            bloom_desktop_close_impl(w);
            return 0;
        case WM_SIZE:
            w->layout_dirty = true;
            break;
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
            w->layout_dirty = true;
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
    w->next_layout_id = BLOOM_DESKTOP_LAYOUT_ID_BASE;
    w->event_head = 0;
    w->event_count = 0;
    UINT dpi = (UINT)w->dpi;
    int px = (int)((width * dpi + 48) / 96);
    int py = (int)((height * dpi + 48) / 96);
    w->hwnd = CreateWindowExW(
        WS_EX_CONTROLPARENT,
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

static bool bloom_desktop_area_text_ok(bloom_str s) {
    if (s.len > BLOOM_DESKTOP_MAX_AREA_TEXT) {
        return false;
    }
    for (size_t i = 0; i < s.len; i++) {
        unsigned char ch = (unsigned char)s.data[i];
        if (ch < 0x20 && ch != '\n' && ch != '\r' && ch != '\t') {
            return false;
        }
        if (ch == 0x7F || (ch == 0 && i < s.len)) {
            return false;
        }
    }
    return s.len == 0 || memchr(s.data, 0, s.len) == NULL;
}

static bloom_str bloom_desktop_to_crlf(bloom_str s) {
    size_t extra = 0;
    for (size_t i = 0; i < s.len; i++) {
        if (s.data[i] == '\n' && (i == 0 || s.data[i - 1] != '\r')) {
            extra++;
        }
    }
    if (extra == 0) {
        return bloom_str_clone(s);
    }
    char *buf = (char *)malloc(s.len + extra + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t n = 0;
    for (size_t i = 0; i < s.len; i++) {
        if (s.data[i] == '\n' && (i == 0 || s.data[i - 1] != '\r')) {
            buf[n++] = '\r';
        }
        buf[n++] = s.data[i];
    }
    bloom_str r = bloom_str_from_bytes(buf, n);
    free(buf);
    return r;
}

static bloom_str bloom_desktop_to_lf(bloom_str s) {
    char *buf = (char *)malloc(s.len + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t n = 0;
    for (size_t i = 0; i < s.len; i++) {
        if (s.data[i] == '\r') {
            if (i + 1 < s.len && s.data[i + 1] == '\n') {
                continue;
            }
            buf[n++] = '\n';
        } else {
            buf[n++] = s.data[i];
        }
    }
    bloom_str r = bloom_str_from_bytes(buf, n);
    free(buf);
    return r;
}

static bloom_str bloom_desktop_get_control_text(
    bloom_desktop_window *w,
    bloom_desktop_control *c
) {
    (void)w;
    bool ok = false;
    wchar_t *text = bloom_desktop_read_text(c->hwnd, &ok);
    if (!ok) {
        free(text);
        return bloom_str_from_bytes("", 0);
    }
    bloom_str s = bloom_desktop_from_utf16(text, (int)wcslen(text), &ok);
    free(text);
    if (!ok) {
        return bloom_str_from_bytes("", 0);
    }
    if (c->kind == 5) {
        bloom_str normalized = bloom_desktop_to_lf(s);
        bloom_str_drop(&s);
        return normalized;
    }
    return s;
}

static bool bloom_desktop_set_control_text(
    bloom_desktop_window *w,
    bloom_desktop_control *c,
    bloom_str text
) {
    bloom_str owned = {0, NULL, 0};
    if (c->kind == 5) {
        owned = bloom_desktop_to_crlf(text);
    }
    wchar_t *wt = bloom_desktop_to_utf16(c->kind == 5 ? owned : text);
    bloom_str_drop(&owned);
    if (wt == NULL) {
        return false;
    }
    size_t limit = c->kind == 5 ? BLOOM_DESKTOP_MAX_AREA_TEXT : BLOOM_DESKTOP_MAX_TEXT;
    if (wcslen(wt) > limit) {
        free(wt);
        return false;
    }
    w->suppress_events++;
    BOOL ok = SetWindowTextW(c->hwnd, wt);
    w->suppress_events--;
    free(wt);
    return ok != 0;
}

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
    bool multiline,
    const char *op
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(handle, op, &err)) {
        return bloom_desktop_err_i64(err);
    }
    if (multiline ? !bloom_desktop_area_text_ok(text) : !bloom_desktop_text_ok(text)) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            op,
            multiline
                ? "text exceeds 65536 bytes or contains invalid control/NUL bytes"
                : "text exceeds 1048576 bytes or contains control/NUL bytes"
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
    bloom_str prepared = {0, NULL, 0};
    if (multiline) {
        prepared = bloom_desktop_to_crlf(text);
    }
    wchar_t *wt = bloom_desktop_to_utf16(multiline ? prepared : text);
    bloom_str_drop(&prepared);
    if (wt == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(op, "text is not valid UTF-8"));
    }
    int64_t id = w->next_control_id++;
    int px = (int)((x * w->dpi + 48) / 96);
    int py = (int)((y * w->dpi + 48) / 96);
    int pw = (int)((width * w->dpi + 48) / 96);
    int ph = (int)((height * w->dpi + 48) / 96);
    w->suppress_events++;
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
    w->suppress_events--;
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
    if (kind == 3 || kind == 5) {
        SendMessageW(
            hwnd,
            EM_SETLIMITTEXT,
            (WPARAM)(kind == 5 ? BLOOM_DESKTOP_MAX_AREA_TEXT : BLOOM_DESKTOP_MAX_TEXT),
            0
        );
    }
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
        false,
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
        false,
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
        false,
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
        false,
        "checkbox_create"
    );
}

bloom_enum_Result_i64_str bloom_desktop_text_area_create(
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
        ES_MULTILINE | ES_WANTRETURN | ES_AUTOVSCROLL | WS_VSCROLL | WS_TABSTOP,
        5,
        true,
        "text_area_create"
    );
}

bloom_enum_Result_i64_str bloom_desktop_combo_create(
    bloom_struct_std_desktop_Window window,
    bloom_str text,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    if (!bloom_desktop_text_ok(text)) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "combo_create",
            "text exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    bloom_str empty = {0, NULL, 0};
    bloom_enum_Result_i64_str created = bloom_desktop_create_control(
        window,
        empty,
        x,
        y,
        width,
        height,
        L"COMBOBOX",
        CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP,
        4,
        false,
        "combo_create"
    );
    if (created.tag != 0 || text.len == 0) {
        return created;
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    if (w->control_count == 0) {
        return created;
    }
    bloom_desktop_control *c = &w->controls[w->control_count - 1];
    wchar_t *wt = bloom_desktop_to_utf16(text);
    if (wt == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "combo_create",
            "text is not valid UTF-8"
        ));
    }
    LRESULT index = SendMessageW(c->hwnd, CB_ADDSTRING, 0, (LPARAM)wt);
    free(wt);
    if (index < 0) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "combo_create",
            "initial combo box item could not be added"
        ));
    }
    SendMessageW(c->hwnd, CB_SETCURSEL, (WPARAM)index, 0);
    return created;
}

bloom_enum_Result_i64_str bloom_desktop_list_create(
    bloom_struct_std_desktop_Window window,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height
) {
    bloom_str empty = {0, NULL, 0};
    return bloom_desktop_create_control(
        window,
        empty,
        x,
        y,
        width,
        height,
        L"LISTBOX",
        LBS_NOTIFY | WS_VSCROLL | WS_BORDER | WS_TABSTOP,
        6,
        false,
        "list_create"
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
    if (c->kind == 6) {
        return bloom_desktop_err_str(bloom_desktop_msg(
            "control_text",
            "list boxes have no single text value; use list_selected_text"
        ));
    }
    return bloom_desktop_ok_str(bloom_desktop_get_control_text(
        bloom_desktop_window_from(window),
        c
    ));
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
    if (c->kind == 4) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "control_set_text",
            "use combo_set_selected to change a combo box"
        ));
    }
    if (c->kind == 6) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "control_set_text",
            "use list_set_selected to change a list box"
        ));
    }
    bool multiline = c->kind == 5;
    if (multiline ? !bloom_desktop_area_text_ok(text) : !bloom_desktop_text_ok(text)) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "control_set_text",
            multiline
                ? "text exceeds 65536 bytes or contains invalid control/NUL bytes"
                : "text exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    if (!bloom_desktop_set_control_text(w, c, text)) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "control_set_text",
            "text is not valid UTF-8 or is too long for this control"
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

/* ---- public: combo boxes and list boxes ------------------------------- */

static bloom_enum_Result_i64_str bloom_desktop_list_value(
    int64_t value,
    const char *op
) {
    (void)op;
    return bloom_desktop_ok_i64(value);
}

bloom_enum_Result_i64_str bloom_desktop_combo_add(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    bloom_str text
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        4,
        "combo_add",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_i64(err);
    }
    if (!bloom_desktop_text_ok(text)) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "combo_add",
            "text exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    wchar_t *wt = bloom_desktop_to_utf16(text);
    if (wt == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "combo_add",
            "text is not valid UTF-8"
        ));
    }
    LRESULT index = SendMessageW(c->hwnd, CB_ADDSTRING, 0, (LPARAM)wt);
    free(wt);
    if (index < 0) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "combo_add",
            "item could not be added"
        ));
    }
    return bloom_desktop_list_value((int64_t)index, "combo_add");
}

bloom_enum_Result_bool_str bloom_desktop_combo_remove(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    int64_t index
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        4,
        "combo_remove",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    int count = (int)SendMessageW(c->hwnd, CB_GETCOUNT, 0, 0);
    if (index < 0 || index >= count) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "combo_remove",
            "index is out of range"
        ));
    }
    if (SendMessageW(c->hwnd, CB_DELETESTRING, (WPARAM)index, 0) == CB_ERR) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "combo_remove",
            "item could not be removed"
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_combo_clear(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        4,
        "combo_clear",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    SendMessageW(c->hwnd, CB_RESETCONTENT, 0, 0);
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_i64_str bloom_desktop_combo_count(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        4,
        "combo_count",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_i64(err);
    }
    return bloom_desktop_list_value(
        (int64_t)SendMessageW(c->hwnd, CB_GETCOUNT, 0, 0),
        "combo_count"
    );
}

bloom_enum_Result_i64_str bloom_desktop_combo_selected(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        4,
        "combo_selected",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_i64(err);
    }
    return bloom_desktop_list_value(
        (int64_t)SendMessageW(c->hwnd, CB_GETCURSEL, 0, 0),
        "combo_selected"
    );
}

bloom_enum_Result_bool_str bloom_desktop_combo_set_selected(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    int64_t index
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        4,
        "combo_set_selected",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    int count = (int)SendMessageW(c->hwnd, CB_GETCOUNT, 0, 0);
    if (index < -1 || index >= count) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "combo_set_selected",
            "index is out of range"
        ));
    }
    if (SendMessageW(c->hwnd, CB_SETCURSEL, (WPARAM)index, 0) == CB_ERR
        && index >= 0) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "combo_set_selected",
            "selection could not be changed"
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_str_str bloom_desktop_combo_selected_text(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        4,
        "combo_selected_text",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_str(err);
    }
    bool ok = false;
    bloom_str s = bloom_desktop_selected_text(c->hwnd, 4, &ok);
    if (!ok) {
        bloom_str_drop(&s);
        return bloom_desktop_err_str(bloom_desktop_msg(
            "combo_selected_text",
            "selected text is too long or not valid Unicode"
        ));
    }
    return bloom_desktop_ok_str(s);
}

bloom_enum_Result_i64_str bloom_desktop_list_add(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    bloom_str text
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        6,
        "list_add",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_i64(err);
    }
    if (!bloom_desktop_text_ok(text)) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "list_add",
            "text exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    wchar_t *wt = bloom_desktop_to_utf16(text);
    if (wt == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "list_add",
            "text is not valid UTF-8"
        ));
    }
    LRESULT index = SendMessageW(c->hwnd, LB_ADDSTRING, 0, (LPARAM)wt);
    free(wt);
    if (index < 0) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "list_add",
            "item could not be added"
        ));
    }
    return bloom_desktop_list_value((int64_t)index, "list_add");
}

bloom_enum_Result_bool_str bloom_desktop_list_remove(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    int64_t index
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        6,
        "list_remove",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    int count = (int)SendMessageW(c->hwnd, LB_GETCOUNT, 0, 0);
    if (index < 0 || index >= count) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "list_remove",
            "index is out of range"
        ));
    }
    if (SendMessageW(c->hwnd, LB_DELETESTRING, (WPARAM)index, 0) == LB_ERR) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "list_remove",
            "item could not be removed"
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_list_clear(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        6,
        "list_clear",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    SendMessageW(c->hwnd, LB_RESETCONTENT, 0, 0);
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_i64_str bloom_desktop_list_count(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        6,
        "list_count",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_i64(err);
    }
    return bloom_desktop_list_value(
        (int64_t)SendMessageW(c->hwnd, LB_GETCOUNT, 0, 0),
        "list_count"
    );
}

bloom_enum_Result_i64_str bloom_desktop_list_selected(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        6,
        "list_selected",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_i64(err);
    }
    return bloom_desktop_list_value(
        (int64_t)SendMessageW(c->hwnd, LB_GETCURSEL, 0, 0),
        "list_selected"
    );
}

bloom_enum_Result_bool_str bloom_desktop_list_set_selected(
    bloom_struct_std_desktop_Window window,
    int64_t control,
    int64_t index
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        6,
        "list_set_selected",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_bool(err);
    }
    int count = (int)SendMessageW(c->hwnd, LB_GETCOUNT, 0, 0);
    if (index < -1 || index >= count) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "list_set_selected",
            "index is out of range"
        ));
    }
    if (SendMessageW(c->hwnd, LB_SETCURSEL, (WPARAM)index, 0) == LB_ERR
        && index >= 0) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "list_set_selected",
            "selection could not be changed"
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_str_str bloom_desktop_list_selected_text(
    bloom_struct_std_desktop_Window window,
    int64_t control
) {
    bloom_str err = {0, NULL, 0};
    bloom_desktop_control *c = bloom_desktop_control_for(
        window,
        control,
        6,
        "list_selected_text",
        &err
    );
    if (c == NULL) {
        return bloom_desktop_err_str(err);
    }
    bool ok = false;
    bloom_str s = bloom_desktop_selected_text(c->hwnd, 6, &ok);
    if (!ok) {
        bloom_str_drop(&s);
        return bloom_desktop_err_str(bloom_desktop_msg(
            "list_selected_text",
            "selected text is too long or not valid Unicode"
        ));
    }
    return bloom_desktop_ok_str(s);
}

bloom_enum_Result_i64_str bloom_desktop_events_dropped(
    bloom_struct_std_desktop_Window window
) {
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    if (w == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "events_dropped",
            "invalid window handle"
        ));
    }
    if (!bloom_desktop_thread_ok(w)) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "events_dropped",
            "desktop calls must run on the window's thread"
        ));
    }
    return bloom_desktop_ok_i64(w->dropped_events);
}

/* ---- public: menus ---------------------------------------------------- */

static bloom_desktop_menu_entry *bloom_desktop_menu_entry_for(
    bloom_desktop_window *w,
    int64_t id
) {
    for (int i = 0; i < w->menu_count; i++) {
        if (w->menu_entries[i].id == id) {
            return &w->menu_entries[i];
        }
    }
    return NULL;
}

static HMENU bloom_desktop_menu_parent(bloom_desktop_window *w, int64_t parent_id) {
    if (parent_id == 0) {
        return w->menu;
    }
    bloom_desktop_menu_entry *entry = bloom_desktop_menu_entry_for(w, parent_id);
    if (entry == NULL || !entry->submenu) {
        return NULL;
    }
    return GetSubMenu(w->menu, entry->position);
}

bloom_enum_Result_bool_str bloom_desktop_menu_create(
    bloom_struct_std_desktop_Window window
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "menu_create", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    if (w->menu != NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "menu_create",
            "window already has a menu bar"
        ));
    }
    HMENU menu = CreateMenu();
    if (menu == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_werr(
            "menu_create",
            GetLastError()
        ));
    }
    if (!SetMenu(w->hwnd, menu)) {
        DestroyMenu(menu);
        return bloom_desktop_err_bool(bloom_desktop_werr(
            "menu_create",
            GetLastError()
        ));
    }
    w->menu = menu;
    w->next_menu_id = BLOOM_DESKTOP_MENU_ID_BASE;
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_i64_str bloom_desktop_menu_add(
    bloom_struct_std_desktop_Window window,
    bloom_str label
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "menu_add", &err)) {
        return bloom_desktop_err_i64(err);
    }
    if (!bloom_desktop_text_ok(label)) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "menu_add",
            "label exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    if (w->menu == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "menu_add",
            "create a menu bar with menu_create first"
        ));
    }
    if (w->menu_count >= BLOOM_DESKTOP_MAX_MENU_ITEMS
        || w->next_menu_id >= BLOOM_DESKTOP_LAYOUT_ID_BASE) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "menu_add",
            "too many menu entries (max 128)"
        ));
    }
    wchar_t *wt = bloom_desktop_to_utf16(label);
    if (wt == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "menu_add",
            "label is not valid UTF-8"
        ));
    }
    HMENU submenu = CreatePopupMenu();
    if (submenu == NULL) {
        free(wt);
        return bloom_desktop_err_i64(bloom_desktop_werr(
            "menu_add",
            GetLastError()
        ));
    }
    int position = GetMenuItemCount(w->menu);
    if (position < 0 || !AppendMenuW(
            w->menu,
            MF_STRING | MF_POPUP,
            (UINT_PTR)submenu,
            wt
        )) {
        free(wt);
        DestroyMenu(submenu);
        return bloom_desktop_err_i64(bloom_desktop_werr(
            "menu_add",
            GetLastError()
        ));
    }
    free(wt);
    int64_t id = w->next_menu_id++;
    bloom_desktop_menu_entry *entry = &w->menu_entries[w->menu_count++];
    entry->id = id;
    entry->parent_id = 0;
    entry->position = position;
    entry->submenu = true;
    entry->label = bloom_str_clone(label);
    return bloom_desktop_ok_i64(id);
}

bloom_enum_Result_i64_str bloom_desktop_menu_item(
    bloom_struct_std_desktop_Window window,
    int64_t menu_id,
    bloom_str label
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "menu_item", &err)) {
        return bloom_desktop_err_i64(err);
    }
    if (!bloom_desktop_text_ok(label)) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "menu_item",
            "label exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    if (w->menu == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "menu_item",
            "window has no menu bar"
        ));
    }
    bloom_desktop_menu_entry *parent = bloom_desktop_menu_entry_for(w, menu_id);
    HMENU hmenu = bloom_desktop_menu_parent(w, menu_id);
    if (parent == NULL || !parent->submenu || hmenu == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "menu_item",
            "menu_id must refer to a popup menu created with menu_add"
        ));
    }
    if (w->menu_count >= BLOOM_DESKTOP_MAX_MENU_ITEMS
        || w->next_menu_id >= BLOOM_DESKTOP_LAYOUT_ID_BASE) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "menu_item",
            "too many menu entries (max 128)"
        ));
    }
    wchar_t *wt = bloom_desktop_to_utf16(label);
    if (wt == NULL) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "menu_item",
            "label is not valid UTF-8"
        ));
    }
    int position = GetMenuItemCount(hmenu);
    int64_t id = w->next_menu_id++;
    if (position < 0 || !AppendMenuW(hmenu, MF_STRING, (UINT_PTR)id, wt)) {
        free(wt);
        return bloom_desktop_err_i64(bloom_desktop_werr(
            "menu_item",
            GetLastError()
        ));
    }
    free(wt);
    bloom_desktop_menu_entry *entry = &w->menu_entries[w->menu_count++];
    entry->id = id;
    entry->parent_id = menu_id;
    entry->position = position;
    entry->submenu = false;
    entry->label = bloom_str_clone(label);
    return bloom_desktop_ok_i64(id);
}

bloom_enum_Result_bool_str bloom_desktop_menu_separator(
    bloom_struct_std_desktop_Window window,
    int64_t menu_id
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "menu_separator", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    bloom_desktop_menu_entry *parent = bloom_desktop_menu_entry_for(w, menu_id);
    HMENU hmenu = bloom_desktop_menu_parent(w, menu_id);
    if (parent == NULL || !parent->submenu || hmenu == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "menu_separator",
            "menu_id must refer to a popup menu created with menu_add"
        ));
    }
    if (!AppendMenuW(hmenu, MF_SEPARATOR, 0, NULL)) {
        return bloom_desktop_err_bool(bloom_desktop_werr(
            "menu_separator",
            GetLastError()
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_menu_set_enabled(
    bloom_struct_std_desktop_Window window,
    int64_t item_id,
    bool enabled
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "menu_set_enabled", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    bloom_desktop_menu_entry *entry = bloom_desktop_menu_entry_for(w, item_id);
    if (entry == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "menu_set_enabled",
            "unknown menu id"
        ));
    }
    HMENU hmenu = bloom_desktop_menu_parent(w, entry->parent_id);
    if (hmenu == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "menu_set_enabled",
            "menu is not available"
        ));
    }
    UINT flags = MF_BYPOSITION | (enabled ? MF_ENABLED : MF_GRAYED);
    if (EnableMenuItem(hmenu, (UINT)entry->position, flags) == (UINT)-1) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "menu_set_enabled",
            "menu item could not be changed"
        ));
    }
    DrawMenuBar(w->hwnd);
    return bloom_desktop_ok_bool();
}

/* ---- public: layout --------------------------------------------------- */

static bloom_desktop_layout *bloom_desktop_layout_for(
    bloom_desktop_window *w,
    int64_t id
) {
    for (int i = 0; i < w->layout_count; i++) {
        if (w->layouts[i].id == id) {
            return &w->layouts[i];
        }
    }
    return NULL;
}

static bool bloom_desktop_align_ok(bloom_str align, int *value) {
    if (align.len == 5 && memcmp(align.data, "start", 5) == 0) {
        *value = 0;
        return true;
    }
    if (align.len == 6 && memcmp(align.data, "center", 6) == 0) {
        *value = 1;
        return true;
    }
    if (align.len == 3 && memcmp(align.data, "end", 3) == 0) {
        *value = 2;
        return true;
    }
    if (align.len == 7 && memcmp(align.data, "stretch", 7) == 0) {
        *value = 3;
        return true;
    }
    return false;
}

static int64_t bloom_desktop_clamp_i32(int64_t value) {
    if (value > 2147483647LL) {
        return 2147483647LL;
    }
    if (value < -2147483648LL) {
        return -2147483648LL;
    }
    return value;
}

static int bloom_desktop_scale(int64_t logical, int dpi) {
    int64_t value = (logical * (int64_t)dpi + 48) / 96;
    return (int)bloom_desktop_clamp_i32(value);
}

bloom_enum_Result_i64_str bloom_desktop_layout_create(
    bloom_struct_std_desktop_Window window,
    bloom_str kind
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "layout_create", &err)) {
        return bloom_desktop_err_i64(err);
    }
    int value = -1;
    if (kind.len == 3 && memcmp(kind.data, "row", 3) == 0) {
        value = 0;
    } else if (kind.len == 6 && memcmp(kind.data, "column", 6) == 0) {
        value = 1;
    }
    if (value < 0) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "layout_create",
            "kind must be \"row\" or \"column\""
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    if (w->layout_count >= BLOOM_DESKTOP_MAX_LAYOUTS) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "layout_create",
            "too many layouts (max 64)"
        ));
    }
    bloom_desktop_layout *layout = &w->layouts[w->layout_count++];
    memset(layout, 0, sizeof *layout);
    layout->id = w->next_layout_id++;
    layout->kind = value;
    layout->align = 0;
    return bloom_desktop_ok_i64(layout->id);
}

static bloom_enum_Result_bool_str bloom_desktop_layout_spacing(
    bloom_struct_std_desktop_Window window,
    int64_t layout_id,
    int64_t value,
    int which,
    const char *op
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, op, &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    bloom_desktop_layout *layout = bloom_desktop_layout_for(w, layout_id);
    if (layout == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(op, "unknown layout id"));
    }
    if (value < 0 || value > 4096) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            op,
            "value must be between 0 and 4096 logical units"
        ));
    }
    if (which == 0) {
        layout->padding = value;
    } else {
        layout->spacing = value;
    }
    w->layout_dirty = true;
    if (w->root_layout != 0) {
        bloom_desktop_layout_apply(w);
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_layout_set_padding(
    bloom_struct_std_desktop_Window window,
    int64_t layout_id,
    int64_t padding
) {
    return bloom_desktop_layout_spacing(
        window,
        layout_id,
        padding,
        0,
        "layout_set_padding"
    );
}

bloom_enum_Result_bool_str bloom_desktop_layout_set_spacing(
    bloom_struct_std_desktop_Window window,
    int64_t layout_id,
    int64_t spacing
) {
    return bloom_desktop_layout_spacing(
        window,
        layout_id,
        spacing,
        1,
        "layout_set_spacing"
    );
}

bloom_enum_Result_bool_str bloom_desktop_layout_set_align(
    bloom_struct_std_desktop_Window window,
    int64_t layout_id,
    bloom_str align
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "layout_set_align", &err)) {
        return bloom_desktop_err_bool(err);
    }
    int value = 0;
    if (!bloom_desktop_align_ok(align, &value)) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_set_align",
            "align must be start, center, end or stretch"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    bloom_desktop_layout *layout = bloom_desktop_layout_for(w, layout_id);
    if (layout == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_set_align",
            "unknown layout id"
        ));
    }
    layout->align = value;
    w->layout_dirty = true;
    if (w->root_layout != 0) {
        bloom_desktop_layout_apply(w);
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_layout_add_control(
    bloom_struct_std_desktop_Window window,
    int64_t layout_id,
    int64_t control,
    bloom_struct_std_desktop_LayoutItem item
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "layout_add_control", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    bloom_desktop_layout *layout = bloom_desktop_layout_for(w, layout_id);
    if (layout == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_control",
            "unknown layout id"
        ));
    }
    if (bloom_desktop_find_control(w, control) == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_control",
            "unknown control id"
        ));
    }
    if (item.f_flex < 0 || item.f_flex > 1000) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_control",
            "flex must be between 0 and 1000"
        ));
    }
    if (item.f_width < 0 || item.f_width > 16384 || item.f_height < 0
        || item.f_height > 16384) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_control",
            "width and height must be between 0 and 16384"
        ));
    }
    int align = -1;
    if (item.f_align.len > 0 && !bloom_desktop_align_ok(item.f_align, &align)) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_control",
            "align must be start, center, end, stretch or empty"
        ));
    }
    if (layout->child_count >= BLOOM_DESKTOP_MAX_LAYOUT_CHILDREN) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_control",
            "too many children in layout (max 256)"
        ));
    }
    if (layout->child_count == layout->child_cap) {
        int cap = layout->child_cap ? layout->child_cap * 2 : 4;
        if (cap > BLOOM_DESKTOP_MAX_LAYOUT_CHILDREN) {
            cap = BLOOM_DESKTOP_MAX_LAYOUT_CHILDREN;
        }
        bloom_desktop_layout_child *grown = (bloom_desktop_layout_child *)realloc(
            layout->children,
            (size_t)cap * sizeof *grown
        );
        if (grown == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        layout->children = grown;
        layout->child_cap = cap;
    }
    bloom_desktop_layout_child *child = &layout->children[layout->child_count++];
    child->type = 0;
    child->control = control;
    child->layout_id = 0;
    child->width = item.f_width;
    child->height = item.f_height;
    child->flex = item.f_flex;
    child->align = align;
    w->layout_dirty = true;
    if (w->root_layout != 0) {
        bloom_desktop_layout_apply(w);
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_layout_add_layout(
    bloom_struct_std_desktop_Window window,
    int64_t parent_id,
    int64_t child_id,
    bloom_struct_std_desktop_LayoutItem item
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "layout_add_layout", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    bloom_desktop_layout *parent = bloom_desktop_layout_for(w, parent_id);
    bloom_desktop_layout *child = bloom_desktop_layout_for(w, child_id);
    if (parent == NULL || child == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_layout",
            "unknown layout id"
        ));
    }
    if (item.f_flex < 0 || item.f_flex > 1000) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_layout",
            "flex must be between 0 and 1000"
        ));
    }
    if (item.f_width < 0 || item.f_width > 16384 || item.f_height < 0
        || item.f_height > 16384) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_layout",
            "width and height must be between 0 and 16384"
        ));
    }
    int align = -1;
    if (item.f_align.len > 0 && !bloom_desktop_align_ok(item.f_align, &align)) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_layout",
            "align must be start, center, end, stretch or empty"
        ));
    }
    if (child->parent_id != 0) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_layout",
            "layout is already attached"
        ));
    }
    if (parent->child_count >= BLOOM_DESKTOP_MAX_LAYOUT_CHILDREN) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_layout",
            "too many children in layout (max 256)"
        ));
    }
    int depth = 1;
    for (bloom_desktop_layout *walk = parent; walk->parent_id != 0; ) {
        walk = bloom_desktop_layout_for(w, walk->parent_id);
        if (walk == NULL) {
            break;
        }
        depth++;
    }
    if (depth > BLOOM_DESKTOP_MAX_LAYOUT_DEPTH) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_add_layout",
            "layout nesting is too deep (max 32)"
        ));
    }
    if (parent->child_count == parent->child_cap) {
        int cap = parent->child_cap ? parent->child_cap * 2 : 4;
        if (cap > BLOOM_DESKTOP_MAX_LAYOUT_CHILDREN) {
            cap = BLOOM_DESKTOP_MAX_LAYOUT_CHILDREN;
        }
        bloom_desktop_layout_child *grown = (bloom_desktop_layout_child *)realloc(
            parent->children,
            (size_t)cap * sizeof *grown
        );
        if (grown == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        parent->children = grown;
        parent->child_cap = cap;
    }
    bloom_desktop_layout_child *slot = &parent->children[parent->child_count++];
    slot->type = 1;
    slot->control = 0;
    slot->layout_id = child_id;
    slot->width = item.f_width;
    slot->height = item.f_height;
    slot->flex = item.f_flex;
    slot->align = align;
    child->parent_id = parent_id;
    w->layout_dirty = true;
    if (w->root_layout != 0) {
        bloom_desktop_layout_apply(w);
    }
    return bloom_desktop_ok_bool();
}

static bool bloom_desktop_layout_place(
    bloom_desktop_window *w,
    bloom_desktop_layout *layout,
    int64_t x,
    int64_t y,
    int64_t width,
    int64_t height,
    int depth,
    int *nodes
) {
    if (depth > BLOOM_DESKTOP_MAX_LAYOUT_DEPTH || *nodes > BLOOM_DESKTOP_MAX_LAYOUT_NODES) {
        return false;
    }
    (*nodes)++;
    int64_t pad = layout->padding;
    if (pad * 2 >= width || pad * 2 >= height) {
        pad = 0;
    }
    int64_t inner_x = x + pad;
    int64_t inner_y = y + pad;
    int64_t inner_w = width - pad * 2;
    int64_t inner_h = height - pad * 2;
    if (inner_w < 0) {
        inner_w = 0;
    }
    if (inner_h < 0) {
        inner_h = 0;
    }
    bool row = layout->kind == 0;
    int64_t main_size = row ? inner_w : inner_h;
    int64_t cross_size = row ? inner_h : inner_w;
    int count = layout->child_count;
    int64_t spacing = layout->spacing;
    int64_t total_spacing = count > 1 ? spacing * (count - 1) : 0;
    if (total_spacing > main_size) {
        total_spacing = main_size;
    }
    int64_t available = main_size - total_spacing;
    if (available < 0) {
        available = 0;
    }
    int64_t fixed_total = 0;
    int64_t flex_total = 0;
    for (int i = 0; i < count; i++) {
        bloom_desktop_layout_child *ch = &layout->children[i];
        int64_t fixed = row ? ch->width : ch->height;
        if (fixed > 0) {
            fixed_total += fixed;
        } else if (ch->type == 1) {
            flex_total += ch->flex > 0 ? ch->flex : 1;
        } else {
            flex_total += ch->flex > 0 ? ch->flex : 1;
        }
    }
    int64_t flex_space = available - fixed_total;
    if (flex_space < 0) {
        flex_space = 0;
    }
    int64_t share = flex_total > 0 ? flex_space / flex_total : 0;
    int64_t remainder = flex_total > 0 ? flex_space % flex_total : 0;
    int64_t cursor = row ? inner_x : inner_y;
    for (int i = 0; i < count; i++) {
        bloom_desktop_layout_child *ch = &layout->children[i];
        int64_t fixed_main = row ? ch->width : ch->height;
        int64_t main = fixed_main;
        if (main <= 0) {
            int64_t weight = ch->flex > 0 ? ch->flex : 1;
            main = share * weight;
            if (remainder > 0) {
                int64_t extra = remainder < weight ? remainder : weight;
                main += extra;
                remainder -= extra;
            }
        }
        if (main < 0) {
            main = 0;
        }
        int64_t fixed_cross = row ? ch->height : ch->width;
        int align = ch->align >= 0 ? ch->align : layout->align;
        int64_t cross = fixed_cross > 0 ? fixed_cross : cross_size;
        int64_t cross_offset = 0;
        if (cross >= cross_size) {
            cross = cross_size;
        } else if (align == 1) {
            cross_offset = (cross_size - cross) / 2;
        } else if (align == 2) {
            cross_offset = cross_size - cross;
        }
        int64_t child_x = row ? cursor : inner_x + cross_offset;
        int64_t child_y = row ? inner_y + cross_offset : cursor;
        int64_t rect_w = row ? main : cross;
        int64_t rect_h = row ? cross : main;
        if (ch->type == 0) {
            bloom_desktop_control *c = bloom_desktop_find_control(w, ch->control);
            if (c != NULL) {
                int px = bloom_desktop_scale(child_x, w->dpi);
                int py = bloom_desktop_scale(child_y, w->dpi);
                int pw = bloom_desktop_scale(rect_w < 1 ? 1 : rect_w, w->dpi);
                int ph = bloom_desktop_scale(rect_h < 1 ? 1 : rect_h, w->dpi);
                SetWindowPos(
                    c->hwnd,
                    NULL,
                    px,
                    py,
                    pw,
                    ph,
                    SWP_NOZORDER | SWP_NOACTIVATE
                );
            }
        } else {
            bloom_desktop_layout *child_layout = bloom_desktop_layout_for(
                w,
                ch->layout_id
            );
            if (child_layout != NULL
                && !bloom_desktop_layout_place(
                    w,
                    child_layout,
                    child_x,
                    child_y,
                    rect_w,
                    rect_h,
                    depth + 1,
                    nodes
                )) {
                return false;
            }
        }
        cursor += main + spacing;
    }
    return true;
}

static bool bloom_desktop_layout_apply(bloom_desktop_window *w) {
    w->layout_dirty = false;
    if (w->root_layout == 0 || w->closed || w->hwnd == NULL) {
        return true;
    }
    bloom_desktop_layout *root = bloom_desktop_layout_for(w, w->root_layout);
    if (root == NULL) {
        return false;
    }
    RECT rect;
    if (!GetClientRect(w->hwnd, &rect)) {
        return false;
    }
    w->dpi = (int)bloom_desktop_dpi_for(w->hwnd);
    int64_t width = (int64_t)(rect.right - rect.left) * 96 / (w->dpi > 0 ? w->dpi : 96);
    int64_t height = (int64_t)(rect.bottom - rect.top) * 96 / (w->dpi > 0 ? w->dpi : 96);
    int nodes = 0;
    return bloom_desktop_layout_place(w, root, 0, 0, width, height, 1, &nodes);
}

bloom_enum_Result_bool_str bloom_desktop_layout_attach(
    bloom_struct_std_desktop_Window window,
    int64_t layout_id
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "layout_attach", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    bloom_desktop_layout *layout = bloom_desktop_layout_for(w, layout_id);
    if (layout == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_attach",
            "unknown layout id"
        ));
    }
    if (layout->parent_id != 0) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_attach",
            "layout is nested; attach the root layout"
        ));
    }
    w->root_layout = layout_id;
    w->layout_dirty = true;
    bloom_desktop_layout_apply(w);
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_layout_refresh(
    bloom_struct_std_desktop_Window window
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "layout_refresh", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    w->layout_dirty = true;
    if (!bloom_desktop_layout_apply(w)) {
        return bloom_desktop_err_bool(bloom_desktop_msg(
            "layout_refresh",
            "layout could not be applied"
        ));
    }
    return bloom_desktop_ok_bool();
}

bloom_enum_Result_bool_str bloom_desktop_layout_detach(
    bloom_struct_std_desktop_Window window
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "layout_detach", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    w->root_layout = 0;
    w->layout_dirty = false;
    return bloom_desktop_ok_bool();
}

/* ---- public: keyboard focus and DPI helper ---------------------------- */

bloom_enum_Result_i64_str bloom_desktop_focused_control(
    bloom_struct_std_desktop_Window window
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "focused_control", &err)) {
        return bloom_desktop_err_i64(err);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    HWND focus = GetFocus();
    if (focus == NULL || GetAncestor(focus, GA_ROOT) != w->hwnd) {
        return bloom_desktop_ok_i64(0);
    }
    for (int i = 0; i < w->control_count; i++) {
        if (w->controls[i].hwnd == focus) {
            return bloom_desktop_ok_i64(w->controls[i].id);
        }
    }
    return bloom_desktop_ok_i64(0);
}

bloom_enum_Result_i64_str bloom_desktop_dpi_scale(
    int64_t value,
    int64_t dpi
) {
    if (value < -1000000 || value > 1000000) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "dpi_scale",
            "value must be between -1000000 and 1000000"
        ));
    }
    if (dpi < 48 || dpi > 960) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "dpi_scale",
            "dpi must be between 48 and 960"
        ));
    }
    bool negative = value < 0;
    int64_t magnitude = negative ? -value : value;
    int64_t scaled = (magnitude * dpi + 48) / 96;
    return bloom_desktop_ok_i64(negative ? -scaled : scaled);
}

/* ---- public: file dialogs --------------------------------------------- */

static wchar_t *bloom_desktop_file_filter(bloom_str filter) {
    bloom_str source = filter;
    if (filter.len == 0) {
        source = bloom_str_from_bytes(
            "Alle Dateien (*.*)|*.*|",
            strlen("Alle Dateien (*.*)|*.*|")
        );
    }
    wchar_t *wide = bloom_desktop_to_utf16(source);
    if (filter.len == 0) {
        bloom_str_drop(&source);
    }
    if (wide == NULL) {
        return NULL;
    }
    for (wchar_t *p = wide; *p != L'\0'; p++) {
        if (*p == L'|') {
            *p = L'\0';
        }
    }
    return wide;
}

static bloom_enum_Result_Option_str_str bloom_desktop_ok_path(bloom_str path) {
    bloom_enum_Result_Option_str_str r;
    r.tag = 0;
    r.data.m_Ok._0.tag = 0;
    r.data.m_Ok._0.data.m_Some._0 = path;
    return r;
}

static bloom_enum_Result_Option_str_str bloom_desktop_cancel_path(void) {
    bloom_enum_Result_Option_str_str r;
    r.tag = 0;
    r.data.m_Ok._0.tag = 1;
    return r;
}

static bloom_enum_Result_Option_str_str bloom_desktop_err_path(bloom_str m) {
    bloom_enum_Result_Option_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_Option_str_str bloom_desktop_file_dialog(
    bloom_struct_std_desktop_Window window,
    bloom_str title,
    bloom_str filter,
    bloom_str default_name,
    bool save,
    const char *op
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, op, &err)) {
        return bloom_desktop_err_path(err);
    }
    if (!bloom_desktop_text_ok(title)) {
        return bloom_desktop_err_path(bloom_desktop_msg(
            op,
            "title exceeds 1048576 bytes or contains control/NUL bytes"
        ));
    }
    if (filter.len > 4096) {
        return bloom_desktop_err_path(bloom_desktop_msg(
            op,
            "filter exceeds 4096 bytes"
        ));
    }
    if (!bloom_desktop_text_ok(default_name) || default_name.len > 4096) {
        return bloom_desktop_err_path(bloom_desktop_msg(
            op,
            "default name exceeds 4096 bytes or contains control/NUL bytes"
        ));
    }
    wchar_t *wt = bloom_desktop_to_utf16(title);
    wchar_t *wf = bloom_desktop_file_filter(filter);
    wchar_t *wn = bloom_desktop_to_utf16(default_name);
    if (wt == NULL || wf == NULL || wn == NULL) {
        free(wt);
        free(wf);
        free(wn);
        return bloom_desktop_err_path(bloom_desktop_msg(
            op,
            "title, filter or default name is not valid UTF-8"
        ));
    }
    wchar_t *buffer = (wchar_t *)calloc(32768, sizeof(wchar_t));
    if (buffer == NULL) {
        free(wt);
        free(wf);
        free(wn);
        bloom_panicf(NULL, 0, "out of memory");
    }
    if (wcslen(wn) < 32767) {
        wcscpy(buffer, wn);
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    OPENFILENAMEW ofn;
    memset(&ofn, 0, sizeof ofn);
    ofn.lStructSize = sizeof ofn;
    ofn.hwndOwner = w->hwnd;
    ofn.lpstrFilter = wf;
    ofn.lpstrFile = buffer;
    ofn.nMaxFile = 32768;
    ofn.lpstrTitle = wt;
    ofn.Flags = OFN_EXPLORER | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST;
    if (save) {
        ofn.Flags |= OFN_OVERWRITEPROMPT;
    } else {
        ofn.Flags |= OFN_FILEMUSTEXIST;
    }
    BOOL ok = save ? GetSaveFileNameW(&ofn) : GetOpenFileNameW(&ofn);
    DWORD dialog_error = CommDlgExtendedError();
    if (!ok) {
        free(wt);
        free(wf);
        free(wn);
        free(buffer);
        if (dialog_error == 0) {
            return bloom_desktop_cancel_path();
        }
        char detail[96];
        snprintf(detail, sizeof detail, "dialog failed with code %lu", (unsigned long)dialog_error);
        return bloom_desktop_err_path(bloom_desktop_msg(op, detail));
    }
    bool utf_ok = false;
    bloom_str path = bloom_desktop_from_utf16(buffer, (int)wcslen(buffer), &utf_ok);
    free(wt);
    free(wf);
    free(wn);
    free(buffer);
    if (!utf_ok) {
        bloom_str_drop(&path);
        return bloom_desktop_err_path(bloom_desktop_msg(
            op,
            "selected path is not valid Unicode"
        ));
    }
    return bloom_desktop_ok_path(path);
}

bloom_enum_Result_Option_str_str bloom_desktop_open_file_dialog(
    bloom_struct_std_desktop_Window window,
    bloom_str title,
    bloom_str filter
) {
    bloom_str empty = {0, NULL, 0};
    return bloom_desktop_file_dialog(
        window,
        title,
        filter,
        empty,
        false,
        "open_file_dialog"
    );
}

bloom_enum_Result_Option_str_str bloom_desktop_save_file_dialog(
    bloom_struct_std_desktop_Window window,
    bloom_str title,
    bloom_str filter,
    bloom_str default_name
) {
    return bloom_desktop_file_dialog(
        window,
        title,
        filter,
        default_name,
        true,
        "save_file_dialog"
    );
}

/* ---- public: internal test hooks -------------------------------------- */

bloom_enum_Result_i64_str bloom_desktop_debug_event_flood(
    bloom_struct_std_desktop_Window window,
    int64_t count
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "debug_event_flood", &err)) {
        return bloom_desktop_err_i64(err);
    }
    if (count < 0 || count > 10000) {
        return bloom_desktop_err_i64(bloom_desktop_msg(
            "debug_event_flood",
            "count must be between 0 and 10000"
        ));
    }
    bloom_desktop_window *w = bloom_desktop_window_from(window);
    int before = w->dropped_events;
    for (int64_t i = 0; i < count; i++) {
        bloom_desktop_push_event(w, 0, 0, bloom_str_from_bytes("", 0), false);
    }
    return bloom_desktop_ok_i64(w->dropped_events - before);
}

typedef struct {
    bloom_desktop_window *window;
    bool rejected;
} bloom_desktop_thread_probe;

static DWORD WINAPI bloom_desktop_thread_probe_main(LPVOID param) {
    bloom_desktop_thread_probe *probe = (bloom_desktop_thread_probe *)param;
    bloom_struct_std_desktop_Window h;
    h.f_id = (bloom_desktop_window_handle)(intptr_t)probe->window;
    bloom_str err = {0, NULL, 0};
    probe->rejected = !bloom_desktop_prepare(h, "debug_cross_thread", &err);
    bloom_str_drop(&err);
    return 0;
}

bloom_enum_Result_bool_str bloom_desktop_debug_cross_thread(
    bloom_struct_std_desktop_Window window
) {
    bloom_str err = {0, NULL, 0};
    if (!bloom_desktop_prepare(window, "debug_cross_thread", &err)) {
        return bloom_desktop_err_bool(err);
    }
    bloom_desktop_thread_probe probe;
    probe.window = bloom_desktop_window_from(window);
    probe.rejected = false;
    HANDLE thread = CreateThread(
        NULL,
        0,
        bloom_desktop_thread_probe_main,
        &probe,
        0,
        NULL
    );
    if (thread == NULL) {
        return bloom_desktop_err_bool(bloom_desktop_werr(
            "debug_cross_thread",
            GetLastError()
        ));
    }
    WaitForSingleObject(thread, INFINITE);
    CloseHandle(thread);
    return bloom_desktop_ok_bool_value(probe.rejected);
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
            if (!w->close_delivered) {
                w->close_delivered = true;
                r.tag = 0;
                r.data.m_Ok._0.f_kind = bloom_str_from_bytes("close", 5);
                r.data.m_Ok._0.f_control = 0;
                r.data.m_Ok._0.f_text = bloom_str_from_bytes("", 0);
                r.data.m_Ok._0.f_checked = false;
                return r;
            }
            r.tag = 1;
            r.data.m_Err._0 = bloom_desktop_msg("next_event", "window is closed");
            return r;
        }
        if (w->layout_dirty) {
            bloom_desktop_layout_apply(w);
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
        if (msg.hwnd != NULL) {
            HWND root = GetAncestor(msg.hwnd, GA_ROOT);
            bloom_desktop_window *owner = root != NULL
                ? (bloom_desktop_window *)(void *)GetWindowLongPtrW(root, GWLP_USERDATA)
                : NULL;
            if (bloom_desktop_is_live(owner) && IsDialogMessageW(root, &msg)) {
                continue;
            }
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
        if (w->layout_dirty) {
            bloom_desktop_layout_apply(w);
        }
        if (w->event_count == 0 && w->closed) {
            if (!w->close_delivered) {
                w->close_delivered = true;
                r.tag = 0;
                r.data.m_Ok._0.f_kind = bloom_str_from_bytes("close", 5);
                r.data.m_Ok._0.f_control = 0;
                r.data.m_Ok._0.f_text = bloom_str_from_bytes("", 0);
                r.data.m_Ok._0.f_checked = false;
                return r;
            }
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
