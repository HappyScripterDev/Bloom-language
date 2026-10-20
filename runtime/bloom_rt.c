#define _CRT_SECURE_NO_WARNINGS
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdarg.h>
#include <string.h>
#include <math.h>
#include <inttypes.h>

#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#endif

/*
 * bloom_str: reference-counted UTF-8 byte string.
 *
 * Layout of a heap string (single allocation):
 *   [ bloom_str header (rc, data, len) | bytes | '\0' ]
 * `data` points to the bytes right after the header, so the header is
 * reachable from a value as `(bloom_str *)(data - sizeof(bloom_str))`.
 *
 * String literals are emitted as static bloom_str values with rc == SIZE_MAX
 * ("immortal"): clone and drop are no-ops for them and their data points to
 * a static C string.
 *
 * Ownership model (see docs/MEMORY_MODEL.md):
 *   - Every Bloom variable slot owns one reference.
 *   - Borrowed values (variables, fields, index reads, literals) must be
 *     cloned when stored into an owning slot.
 *   - Heap values are released with bloom_str_drop, which is idempotent.
 */
typedef struct { size_t rc; char *data; size_t len; } bloom_str;

/*
 * Handle to a reference-counted JSON value (std::json). The runtime
 * implementation (bloom_json.c) is emitted only when `std::json` is loaded;
 * the handle type itself is always available because the built-in
 * `std::json::JsonValue` struct stores one.
 */
typedef int64_t bloom_json_handle;

/*
 * Handle to a reference-counted router (std::web); same idea as the JSON
 * handle, implemented in bloom_web.c.
 */
typedef int64_t bloom_web_handle;

/*
 * Handle to a reference-counted parsed template (std::html); implemented in
 * bloom_html.c.
 */
typedef int64_t bloom_html_handle;

/*
 * Handle to a reference-counted byte buffer (std::fs::Bytes); implemented in
 * bloom_fs.c.
 */
typedef int64_t bloom_fs_handle;

/*
 * Calling convention for `extern "system"` declarations. On 32-bit Windows
 * this is `__stdcall`; on x64 and other targets it is the default
 * convention (the macro is empty there).
 */
#if defined(_WIN32) && !defined(_WIN64)
#define BLOOM_SYSTEM_ABI __stdcall
#else
#define BLOOM_SYSTEM_ABI
#endif

#define BLOOM_STR_IMMORTAL SIZE_MAX

size_t bloom_live_allocs = 0;

/* Open network handles (sockets + TLS sessions); maintained by the network
 * runtime, reported next to the allocation counter. */
size_t bloom_live_sockets = 0;

void bloom_mem_report(void) {
    const char *e = getenv("BLOOM_MEM_REPORT");
    if (e != NULL && e[0] != '\0' && e[0] != '0') {
        fprintf(
            stderr,
            "live allocations: %zu, live sockets: %zu\n",
            bloom_live_allocs,
            bloom_live_sockets
        );
    }
}

#define BLOOM_DEFINE_ARR_TYPE(SUF, T) \
typedef struct bloom_arr_##SUF##_s { size_t rc; T *data; size_t len; size_t cap; size_t gen; } bloom_arr_##SUF##_s; \
typedef bloom_arr_##SUF##_s *bloom_arr_##SUF;

BLOOM_DEFINE_ARR_TYPE(i64, int64_t)
BLOOM_DEFINE_ARR_TYPE(f64, double)
BLOOM_DEFINE_ARR_TYPE(bool, bool)
BLOOM_DEFINE_ARR_TYPE(str, bloom_str)
BLOOM_DEFINE_ARR_TYPE(char, uint32_t)

/*
 * Slice views: a view references a (stable) array header, a start offset
 * and its own length, plus the header's layout generation at creation
 * time. All element access goes through the header, so views stay valid
 * when the element buffer is reallocated by growth. Structural mutations
 * (insert/remove/pop) bump the header generation and invalidate views.
 */
#define BLOOM_DEFINE_SLICE_TYPE(SUF, T) \
typedef struct { bloom_arr_##SUF hdr; int64_t start; size_t len; size_t gen; } bloom_slice_##SUF;

BLOOM_DEFINE_SLICE_TYPE(i64, int64_t)
BLOOM_DEFINE_SLICE_TYPE(f64, double)
BLOOM_DEFINE_SLICE_TYPE(bool, bool)
BLOOM_DEFINE_SLICE_TYPE(str, bloom_str)
BLOOM_DEFINE_SLICE_TYPE(char, uint32_t)

void bloom_panicf(const char *file, int64_t line, const char *fmt, ...) {
    fflush(stdout);
    fputs("Bloom runtime error: ", stderr);
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    if (file != NULL) {
        fprintf(stderr, "\n  --> %s:%" PRId64 "\n", file, line);
    } else {
        fputc('\n', stderr);
    }
    fflush(stderr);
    exit(70);
}

void bloom_init(void) {
#ifdef _WIN32
    SetConsoleOutputCP(65001);
    SetConsoleCP(65001);
#endif
}

/* Flushes stdout; used by the print helpers when stdout is not a console,
 * so piped output appears line by line (client/server programs print ports
 * and prompts before blocking). */
void bloom_flush_if_piped(void) {
#ifdef _WIN32
    if (GetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), &(DWORD){0}) == 0) {
        fflush(stdout);
    }
#else
    if (!isatty(fileno(stdout))) {
        fflush(stdout);
    }
#endif
}

/*
 * panic(msg): intentional fatal error. Prints the message with the source
 * location and terminates with exit code 70 (same code as runtime errors).
 * No cleanup paths run after a panic; the process exits directly.
 */
void bloom_panic_msg(const char *file, int64_t line, bloom_str msg) {
    fflush(stdout);
    fputs("Bloom panic: ", stderr);
    if (msg.len > 0) {
        fwrite(msg.data, 1, msg.len, stderr);
    }
    if (file != NULL) {
        fprintf(stderr, "\n  --> %s:%" PRId64 "\n", file, line);
    } else {
        fputc('\n', stderr);
    }
    fflush(stderr);
    exit(70);
}

static int bloom_utf8_encode(uint32_t cp, char *out) {
    if (cp > 0x10FFFFu || (cp >= 0xD800u && cp <= 0xDFFFu)) {
        out[0] = '?';
        return 1;
    }
    if (cp < 0x80u) {
        out[0] = (char)cp;
        return 1;
    }
    if (cp < 0x800u) {
        out[0] = (char)(0xC0u | (cp >> 6));
        out[1] = (char)(0x80u | (cp & 0x3Fu));
        return 2;
    }
    if (cp < 0x10000u) {
        out[0] = (char)(0xE0u | (cp >> 12));
        out[1] = (char)(0x80u | ((cp >> 6) & 0x3Fu));
        out[2] = (char)(0x80u | (cp & 0x3Fu));
        return 3;
    }
    out[0] = (char)(0xF0u | (cp >> 18));
    out[1] = (char)(0x80u | ((cp >> 12) & 0x3Fu));
    out[2] = (char)(0x80u | ((cp >> 6) & 0x3Fu));
    out[3] = (char)(0x80u | (cp & 0x3Fu));
    return 4;
}

static int bloom_utf8_decode(const unsigned char *s, size_t len, uint32_t *out) {
    if (len == 0) {
        return 0;
    }
    unsigned char b0 = s[0];
    if (b0 < 0x80) {
        *out = b0;
        return 1;
    }
    if ((b0 & 0xE0u) == 0xC0u) {
        if (b0 < 0xC2u || len < 2 || (s[1] & 0xC0u) != 0x80u) {
            return 0;
        }
        *out = ((uint32_t)(b0 & 0x1Fu) << 6) | (uint32_t)(s[1] & 0x3Fu);
        return 2;
    }
    if ((b0 & 0xF0u) == 0xE0u) {
        if (len < 3 || (s[1] & 0xC0u) != 0x80u || (s[2] & 0xC0u) != 0x80u) {
            return 0;
        }
        if (b0 == 0xE0u && s[1] < 0xA0u) {
            return 0;
        }
        if (b0 == 0xEDu && s[1] > 0x9Fu) {
            return 0;
        }
        *out = ((uint32_t)(b0 & 0x0Fu) << 12) | ((uint32_t)(s[1] & 0x3Fu) << 6) | (uint32_t)(s[2] & 0x3Fu);
        return 3;
    }
    if ((b0 & 0xF8u) == 0xF0u) {
        if (b0 > 0xF4u || len < 4 || (s[1] & 0xC0u) != 0x80u || (s[2] & 0xC0u) != 0x80u || (s[3] & 0xC0u) != 0x80u) {
            return 0;
        }
        if (b0 == 0xF0u && s[1] < 0x90u) {
            return 0;
        }
        if (b0 == 0xF4u && s[1] > 0x8Fu) {
            return 0;
        }
        *out = ((uint32_t)(b0 & 0x07u) << 18) | ((uint32_t)(s[1] & 0x3Fu) << 12) | ((uint32_t)(s[2] & 0x3Fu) << 6) | (uint32_t)(s[3] & 0x3Fu);
        return 4;
    }
    return 0;
}

bloom_str bloom_str_from_bytes(const char *p, size_t n) {
    bloom_str *b = (bloom_str *)malloc(sizeof(bloom_str) + n + 1);
    if (b == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    b->rc = 1;
    b->data = (char *)(b + 1);
    b->len = n;
    if (n > 0) {
        memcpy(b->data, p, n);
    }
    b->data[n] = '\0';
    bloom_live_allocs++;
    return *b;
}

bloom_str bloom_str_clone(bloom_str s) {
    if (s.data != NULL && s.rc != BLOOM_STR_IMMORTAL) {
        bloom_str *b = (bloom_str *)(void *)(s.data - sizeof(bloom_str));
        b->rc++;
    }
    return s;
}

void bloom_str_drop(bloom_str *s) {
    if (s->data != NULL && s->rc != BLOOM_STR_IMMORTAL) {
        bloom_str *b = (bloom_str *)(void *)(s->data - sizeof(bloom_str));
        if (b->rc > 0) {
            b->rc--;
            if (b->rc == 0) {
                free(b);
                bloom_live_allocs--;
            }
        }
    }
    s->rc = 0;
    s->data = NULL;
    s->len = 0;
}

bloom_str bloom_str_from_cstr(const char *p) {
    return bloom_str_from_bytes(p, strlen(p));
}

bloom_str bloom_str_concat(bloom_str a, bloom_str b) {
    size_t n = a.len + b.len;
    bloom_str *p = (bloom_str *)malloc(sizeof(bloom_str) + n + 1);
    if (p == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    p->rc = 1;
    p->data = (char *)(p + 1);
    p->len = n;
    if (a.len > 0) {
        memcpy(p->data, a.data, a.len);
    }
    if (b.len > 0) {
        memcpy(p->data + a.len, b.data, b.len);
    }
    p->data[n] = '\0';
    bloom_live_allocs++;
    return *p;
}

bool bloom_str_eq(bloom_str a, bloom_str b) {
    return a.len == b.len && (a.len == 0 || memcmp(a.data, b.data, a.len) == 0);
}

int64_t bloom_str_len(bloom_str s) {
    int64_t n = 0;
    for (size_t i = 0; i < s.len; i++) {
        if (((unsigned char)s.data[i] & 0xC0u) != 0x80u) {
            n++;
        }
    }
    return n;
}

bool bloom_str_contains(bloom_str s, bloom_str sub) {
    if (sub.len == 0) {
        return true;
    }
    if (sub.len > s.len) {
        return false;
    }
    for (size_t i = 0; i + sub.len <= s.len; i++) {
        if (memcmp(s.data + i, sub.data, sub.len) == 0) {
            return true;
        }
    }
    return false;
}

bool bloom_str_starts_with(bloom_str s, bloom_str prefix) {
    if (prefix.len > s.len) {
        return false;
    }
    return prefix.len == 0 || memcmp(s.data, prefix.data, prefix.len) == 0;
}

bool bloom_str_ends_with(bloom_str s, bloom_str suffix) {
    if (suffix.len > s.len) {
        return false;
    }
    return suffix.len == 0 || memcmp(s.data + (s.len - suffix.len), suffix.data, suffix.len) == 0;
}

bloom_str bloom_str_trim(bloom_str s) {
    size_t start = 0;
    size_t end = s.len;
    while (start < end && (s.data[start] == ' ' || s.data[start] == '\t' || s.data[start] == '\r' || s.data[start] == '\n')) {
        start++;
    }
    while (end > start && (s.data[end - 1] == ' ' || s.data[end - 1] == '\t' || s.data[end - 1] == '\r' || s.data[end - 1] == '\n')) {
        end--;
    }
    return bloom_str_from_bytes(s.data + start, end - start);
}

bloom_str bloom_str_replace(bloom_str s, bloom_str from, bloom_str to) {
    if (from.len == 0) {
        return bloom_str_from_bytes(s.data, s.len);
    }
    size_t data_cap = s.len + to.len + 1;
    bloom_str *b = (bloom_str *)malloc(sizeof(bloom_str) + data_cap + 1);
    if (b == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    b->rc = 1;
    b->data = (char *)(b + 1);
    b->len = 0;
    size_t used = 0;
    size_t i = 0;
    while (i < s.len) {
        if (i + from.len <= s.len && memcmp(s.data + i, from.data, from.len) == 0) {
            if (used + to.len > data_cap) {
                data_cap = (used + to.len) * 2;
                bloom_str *nb = (bloom_str *)realloc(b, sizeof(bloom_str) + data_cap + 1);
                if (nb == NULL) {
                    bloom_panicf(NULL, 0, "out of memory");
                }
                b = nb;
                b->data = (char *)(b + 1);
            }
            if (to.len > 0) {
                memcpy(b->data + used, to.data, to.len);
            }
            used += to.len;
            i += from.len;
        } else {
            if (used + 1 > data_cap) {
                data_cap = data_cap * 2;
                bloom_str *nb = (bloom_str *)realloc(b, sizeof(bloom_str) + data_cap + 1);
                if (nb == NULL) {
                    bloom_panicf(NULL, 0, "out of memory");
                }
                b = nb;
                b->data = (char *)(b + 1);
            }
            b->data[used] = s.data[i];
            used++;
            i++;
        }
    }
    b->data[used] = '\0';
    b->len = used;
    bloom_live_allocs++;
    return *b;
}

bloom_str bloom_str_to_upper(bloom_str s) {
    bloom_str result = bloom_str_from_bytes(s.data, s.len);
    for (size_t i = 0; i < result.len; i++) {
        if (result.data[i] >= 'a' && result.data[i] <= 'z') {
            result.data[i] = (char)(result.data[i] - 32);
        }
    }
    return result;
}

bloom_str bloom_str_to_lower(bloom_str s) {
    bloom_str result = bloom_str_from_bytes(s.data, s.len);
    for (size_t i = 0; i < result.len; i++) {
        if (result.data[i] >= 'A' && result.data[i] <= 'Z') {
            result.data[i] = (char)(result.data[i] + 32);
        }
    }
    return result;
}


uint32_t bloom_str_char_at(bloom_str s, int64_t idx, const char *file, int64_t line) {
    if (idx < 0) {
        bloom_panicf(file, line, "string index out of bounds (index %" PRId64 ")", idx);
    }
    size_t i = 0;
    int64_t n = 0;
    while (i < s.len) {
        uint32_t cp = 0;
        int used = bloom_utf8_decode((const unsigned char *)s.data + i, s.len - i, &cp);
        if (used <= 0) {
            bloom_panicf(file, line, "invalid UTF-8 in string value");
        }
        if (n == idx) {
            return cp;
        }
        i += (size_t)used;
        n++;
    }
    bloom_panicf(file, line, "string index out of bounds (index %" PRId64 ", length %" PRId64 ")", idx, n);
    return 0;
}

void bloom_format_f64(double x, char *buf, size_t bufsize) {
    if (isnan(x)) {
        snprintf(buf, bufsize, "nan");
        return;
    }
    if (isinf(x)) {
        snprintf(buf, bufsize, x < 0 ? "-inf" : "inf");
        return;
    }
    snprintf(buf, bufsize, "%.15g", x);
    bool has_marker = false;
    for (char *p = buf; *p != '\0'; p++) {
        if (*p == '.' || *p == 'e' || *p == 'E') {
            has_marker = true;
            break;
        }
    }
    if (!has_marker) {
        size_t n = strlen(buf);
        if (n + 2 < bufsize) {
            buf[n] = '.';
            buf[n + 1] = '0';
            buf[n + 2] = '\0';
        }
    }
}

bloom_str bloom_str_from_i64(int64_t v) {
    char buf[32];
    int n = snprintf(buf, sizeof buf, "%" PRId64, v);
    return bloom_str_from_bytes(buf, (size_t)n);
}

bloom_str bloom_str_from_f64(double v) {
    char buf[64];
    bloom_format_f64(v, buf, sizeof buf);
    return bloom_str_from_cstr(buf);
}

bloom_str bloom_str_from_bool(bool v) {
    return bloom_str_from_cstr(v ? "true" : "false");
}

bloom_str bloom_str_from_char(uint32_t c) {
    char buf[4];
    int n = bloom_utf8_encode(c, buf);
    return bloom_str_from_bytes(buf, (size_t)n);
}

int64_t bloom_str_to_i64(bloom_str s, const char *file, int64_t line) {
    size_t i = 0;
    while (i < s.len && (s.data[i] == ' ' || s.data[i] == '\t' || s.data[i] == '\r' || s.data[i] == '\n')) {
        i++;
    }
    bool negative = false;
    if (i < s.len && (s.data[i] == '+' || s.data[i] == '-')) {
        negative = s.data[i] == '-';
        i++;
    }
    if (i >= s.len) {
        bloom_panicf(file, line, "invalid integer: \"%.60s\"", s.data);
    }
    uint64_t acc = 0;
    uint64_t limit = negative ? 9223372036854775808ull : 9223372036854775807ull;
    bool any = false;
    while (i < s.len && s.data[i] >= '0' && s.data[i] <= '9') {
        any = true;
        uint64_t digit = (uint64_t)(s.data[i] - '0');
        if (acc > (limit - digit) / 10u) {
            bloom_panicf(file, line, "integer out of range: \"%.60s\"", s.data);
        }
        acc = acc * 10u + digit;
        i++;
    }
    while (i < s.len && (s.data[i] == ' ' || s.data[i] == '\t' || s.data[i] == '\r' || s.data[i] == '\n')) {
        i++;
    }
    if (!any || i != s.len) {
        bloom_panicf(file, line, "invalid integer: \"%.60s\"", s.data);
    }
    if (negative) {
        if (acc == 9223372036854775808ull) {
            return INT64_MIN;
        }
        return -(int64_t)acc;
    }
    return (int64_t)acc;
}

double bloom_str_to_f64(bloom_str s, const char *file, int64_t line) {
    if (s.len >= 512) {
        bloom_panicf(file, line, "invalid float: value too long");
    }
    const char *start = s.data;
    while (*start == ' ' || *start == '\t' || *start == '\r' || *start == '\n') {
        start++;
    }
    char *end = NULL;
    double value = strtod(start, &end);
    if (end == start) {
        bloom_panicf(file, line, "invalid float: \"%.60s\"", s.data);
    }
    while (*end == ' ' || *end == '\t' || *end == '\r' || *end == '\n') {
        end++;
    }
    if (*end != '\0') {
        bloom_panicf(file, line, "invalid float: \"%.60s\"", s.data);
    }
    return value;
}

uint32_t bloom_char_from_i64(int64_t v, const char *file, int64_t line) {
    if (v < 0 || v > 0x10FFFF || (v >= 0xD800 && v <= 0xDFFF)) {
        bloom_panicf(file, line, "invalid character code point %" PRId64, v);
    }
    return (uint32_t)v;
}

int64_t bloom_iadd(int64_t a, int64_t b) {
    return (int64_t)((uint64_t)a + (uint64_t)b);
}

int64_t bloom_isub(int64_t a, int64_t b) {
    return (int64_t)((uint64_t)a - (uint64_t)b);
}

int64_t bloom_imul(int64_t a, int64_t b) {
    return (int64_t)((uint64_t)a * (uint64_t)b);
}

int64_t bloom_ineg(int64_t a) {
    return (int64_t)(0u - (uint64_t)a);
}

int64_t bloom_idiv(int64_t a, int64_t b, const char *file, int64_t line) {
    if (b == 0) {
        bloom_panicf(file, line, "integer division by zero");
    }
    if (a == INT64_MIN && b == -1) {
        return INT64_MIN;
    }
    return a / b;
}

int64_t bloom_imod(int64_t a, int64_t b, const char *file, int64_t line) {
    if (b == 0) {
        bloom_panicf(file, line, "integer modulo by zero");
    }
    if (a == INT64_MIN && b == -1) {
        return 0;
    }
    return a % b;
}

int64_t bloom_iabs(int64_t x) {
    return x < 0 ? (int64_t)(0u - (uint64_t)x) : x;
}

int64_t bloom_i64_min(int64_t a, int64_t b) {
    return a < b ? a : b;
}

int64_t bloom_i64_max(int64_t a, int64_t b) {
    return a > b ? a : b;
}

int64_t bloom_i64_clamp(int64_t v, int64_t lo, int64_t hi, const char *file, int64_t line) {
    if (lo > hi) {
        bloom_panicf(file, line, "clamp: lower bound %" PRId64 " is greater than upper bound %" PRId64, lo, hi);
    }
    if (v < lo) {
        return lo;
    }
    if (v > hi) {
        return hi;
    }
    return v;
}

double bloom_f64_clamp(double v, double lo, double hi, const char *file, int64_t line) {
    if (lo > hi) {
        bloom_panicf(file, line, "clamp: lower bound is greater than upper bound");
    }
    if (v < lo) {
        return lo;
    }
    if (v > hi) {
        return hi;
    }
    return v;
}

double bloom_math_round(double x) {
    if (isnan(x) || isinf(x)) {
        return x;
    }
    return round(x);
}

int64_t bloom_f64_to_i64(double x, const char *file, int64_t line) {
    if (isnan(x)) {
        bloom_panicf(file, line, "cannot convert NaN to int");
    }
    if (x >= 9223372036854775808.0 || x < -9223372036854775808.0) {
        bloom_panicf(file, line, "float value out of range for int");
    }
    return (int64_t)x;
}

void bloom_print_i64(int64_t v) {
    printf("%" PRId64, v);
}

void bloom_println_i64(int64_t v) {
    printf("%" PRId64 "\n", v);
    bloom_flush_if_piped();
}

void bloom_print_f64(double v) {
    char buf[64];
    bloom_format_f64(v, buf, sizeof buf);
    fputs(buf, stdout);
}

void bloom_println_f64(double v) {
    bloom_print_f64(v);
    fputc('\n', stdout);
    bloom_flush_if_piped();
}

void bloom_print_bool(bool v) {
    fputs(v ? "true" : "false", stdout);
}

void bloom_println_bool(bool v) {
    bloom_print_bool(v);
    fputc('\n', stdout);
    bloom_flush_if_piped();
}

void bloom_print_str(bloom_str s) {
    if (s.len > 0) {
        fwrite(s.data, 1, s.len, stdout);
    }
}

void bloom_println_str(bloom_str s) {
    bloom_print_str(s);
    fputc('\n', stdout);
    bloom_flush_if_piped();
}

void bloom_print_char(uint32_t c) {
    char buf[4];
    int n = bloom_utf8_encode(c, buf);
    fwrite(buf, 1, (size_t)n, stdout);
}

void bloom_println_char(uint32_t c) {
    bloom_print_char(c);
    fputc('\n', stdout);
    bloom_flush_if_piped();
}

void bloom_println_empty(void) {
    fputc('\n', stdout);
    bloom_flush_if_piped();
}

bloom_str bloom_input(bloom_str prompt) {
    bloom_print_str(prompt);
    fflush(stdout);
    size_t data_cap = 128;
    bloom_str *b = (bloom_str *)malloc(sizeof(bloom_str) + data_cap + 1);
    if (b == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    b->rc = 1;
    b->data = (char *)(b + 1);
    b->len = 0;
    size_t len = 0;
    int c;
    while ((c = fgetc(stdin)) != EOF) {
        if (c == '\n') {
            break;
        }
        if (len + 1 > data_cap) {
            data_cap *= 2;
            bloom_str *nb = (bloom_str *)realloc(b, sizeof(bloom_str) + data_cap + 1);
            if (nb == NULL) {
                bloom_panicf(NULL, 0, "out of memory");
            }
            b = nb;
            b->data = (char *)(b + 1);
        }
        b->data[len] = (char)c;
        len++;
    }
    if (len > 0 && b->data[len - 1] == '\r') {
        len--;
    }
    b->data[len] = '\0';
    b->len = len;
    bloom_live_allocs++;
    return *b;
}

void bloom_exit(int64_t code) {
    fflush(stdout);
    exit((int)code);
}

#define BLOOM_CLONE_NONE(x) (x)
#define BLOOM_DROP_NONE(p) ((void)(p))

/*
 * Array values are reference-counted headers (`rc`). Copies of an array
 * value share the header; the last drop frees the element buffer (dropping
 * managed elements such as strings) and the header.
 *
 * The header is stable: growing the array reallocates only the element
 * buffer (`data`); the header itself never moves. Slice views therefore
 * reference the header (never the raw buffer) and stay valid across
 * reallocations.
 *
 * `gen` is the layout generation. Structural mutations that shift element
 * positions (insert/remove/pop) increment it; element writes and `push`
 * do not. Slice views store the generation at creation and validate it on
 * every access.
 *
 * Element ownership:
 *   - push/insert/set clone the incoming element (the array gains a reference).
 *   - pop/remove move the element out (ownership is transferred to the result).
 */
#define BLOOM_DEFINE_ARR_FUNCS(SUF, T, CLONE_ELEM, DROP_ELEM) \
bloom_arr_##SUF bloom_arr_##SUF##_new(void) { \
    bloom_arr_##SUF a = (bloom_arr_##SUF)malloc(sizeof(bloom_arr_##SUF##_s)); \
    if (a == NULL) { \
        bloom_panicf(NULL, 0, "out of memory"); \
    } \
    a->rc = 1; \
    a->data = NULL; \
    a->len = 0; \
    a->cap = 0; \
    a->gen = 0; \
    bloom_live_allocs++; \
    return a; \
} \
bloom_arr_##SUF bloom_arr_##SUF##_clone(bloom_arr_##SUF a) { \
    if (a != NULL) { \
        a->rc++; \
    } \
    return a; \
} \
void bloom_arr_##SUF##_drop(bloom_arr_##SUF *a) { \
    if (a != NULL && *a != NULL) { \
        bloom_arr_##SUF p = *a; \
        if (p->rc > 0) { \
            p->rc--; \
            if (p->rc == 0) { \
                for (size_t i = 0; i < p->len; i++) { \
                    DROP_ELEM(&p->data[i]); \
                } \
                free(p->data); \
                free(p); \
                bloom_live_allocs--; \
            } \
        } \
        *a = NULL; \
    } \
} \
void bloom_arr_##SUF##_reserve(bloom_arr_##SUF a, size_t need) { \
    if (need <= a->cap) { \
        return; \
    } \
    size_t cap = a->cap ? a->cap * 2 : 4; \
    if (cap < need) { \
        cap = need; \
    } \
    T *nd = (T *)realloc(a->data, cap * sizeof(T)); \
    if (nd == NULL) { \
        bloom_panicf(NULL, 0, "out of memory"); \
    } \
    a->data = nd; \
    a->cap = cap; \
} \
void bloom_arr_##SUF##_push(bloom_arr_##SUF a, T v) { \
    bloom_arr_##SUF##_reserve(a, a->len + 1); \
    a->data[a->len] = CLONE_ELEM(v); \
    a->len++; \
} \
void bloom_arr_##SUF##_push_owned(bloom_arr_##SUF a, T v) { \
    bloom_arr_##SUF##_reserve(a, a->len + 1); \
    a->data[a->len] = v; \
    a->len++; \
} \
T bloom_arr_##SUF##_get(bloom_arr_##SUF a, int64_t i, const char *file, int64_t line) { \
    if (i < 0 || (uint64_t)i >= (uint64_t)a->len) { \
        bloom_panicf(file, line, "array index out of bounds (index %" PRId64 ", length %zu)", i, a->len); \
    } \
    return a->data[i]; \
} \
void bloom_arr_##SUF##_set(bloom_arr_##SUF a, int64_t i, T v, const char *file, int64_t line) { \
    if (i < 0 || (uint64_t)i >= (uint64_t)a->len) { \
        bloom_panicf(file, line, "array index out of bounds (index %" PRId64 ", length %zu)", i, a->len); \
    } \
    DROP_ELEM(&a->data[i]); \
    a->data[i] = CLONE_ELEM(v); \
} \
void bloom_arr_##SUF##_insert(bloom_arr_##SUF a, int64_t i, T v, const char *file, int64_t line) { \
    if (i < 0 || (uint64_t)i > (uint64_t)a->len) { \
        bloom_panicf(file, line, "array insert index out of bounds (index %" PRId64 ", length %zu)", i, a->len); \
    } \
    bloom_arr_##SUF##_reserve(a, a->len + 1); \
    if ((uint64_t)i < (uint64_t)a->len) { \
        memmove(a->data + i + 1, a->data + i, (a->len - (size_t)i) * sizeof(T)); \
    } \
    a->data[i] = CLONE_ELEM(v); \
    a->len++; \
    a->gen++; \
} \
T bloom_arr_##SUF##_remove(bloom_arr_##SUF a, int64_t i, const char *file, int64_t line) { \
    if (i < 0 || (uint64_t)i >= (uint64_t)a->len) { \
        bloom_panicf(file, line, "array remove index out of bounds (index %" PRId64 ", length %zu)", i, a->len); \
    } \
    T out = a->data[i]; \
    if ((size_t)i + 1 < a->len) { \
        memmove(a->data + i, a->data + i + 1, (a->len - (size_t)i - 1) * sizeof(T)); \
    } \
    a->len--; \
    a->gen++; \
    return out; \
} \
bloom_arr_##SUF bloom_arr_##SUF##_slice(bloom_arr_##SUF a, int64_t start, int64_t end, const char *file, int64_t line) { \
    if (start < 0 || end < start || (uint64_t)end > (uint64_t)a->len) { \
        bloom_panicf(file, line, "slice bounds out of range (start %" PRId64 ", end %" PRId64 ", length %zu)", start, end, a->len); \
    } \
    bloom_arr_##SUF r = bloom_arr_##SUF##_new(); \
    bloom_arr_##SUF##_reserve(r, (size_t)(end - start)); \
    for (int64_t i = start; i < end; i++) { \
        r->data[r->len] = CLONE_ELEM(a->data[i]); \
        r->len++; \
    } \
    return r; \
}

BLOOM_DEFINE_ARR_FUNCS(i64, int64_t, BLOOM_CLONE_NONE, BLOOM_DROP_NONE)
BLOOM_DEFINE_ARR_FUNCS(f64, double, BLOOM_CLONE_NONE, BLOOM_DROP_NONE)
BLOOM_DEFINE_ARR_FUNCS(bool, bool, BLOOM_CLONE_NONE, BLOOM_DROP_NONE)
BLOOM_DEFINE_ARR_FUNCS(str, bloom_str, bloom_str_clone, bloom_str_drop)
BLOOM_DEFINE_ARR_FUNCS(char, uint32_t, BLOOM_CLONE_NONE, BLOOM_DROP_NONE)

#define BLOOM_DEFINE_SLICE_FUNCS(SUF, T, CLONE_ELEM, DROP_ELEM) \
static void bloom_slice_##SUF##_check(bloom_slice_##SUF s, int64_t i, const char *file, int64_t line, const char *what) { \
    if (s.hdr == NULL) { \
        bloom_panicf(file, line, "slice view is uninitialized (%s access)", what); \
    } \
    if (s.hdr->gen != s.gen) { \
        bloom_panicf(file, line, "slice view was invalidated by a structural change (insert/remove/pop)"); \
    } \
    if (i < 0 || (uint64_t)i >= (uint64_t)s.len) { \
        bloom_panicf(file, line, "slice index out of bounds (index %" PRId64 ", view length %zu)", i, s.len); \
    } \
    if (s.start + (uint64_t)i >= (uint64_t)s.hdr->len) { \
        bloom_panicf(file, line, "slice view is out of date with its array"); \
    } \
} \
bloom_slice_##SUF bloom_slice_##SUF##_new(bloom_arr_##SUF a, int64_t start, int64_t end, const char *file, int64_t line) { \
    if (a == NULL) { \
        bloom_panicf(file, line, "slice_view: invalid array value"); \
    } \
    if (start < 0 || end < start || (uint64_t)end > (uint64_t)a->len) { \
        bloom_panicf(file, line, "slice_view bounds out of range (start %" PRId64 ", end %" PRId64 ", length %zu)", start, end, a->len); \
    } \
    a->rc++; \
    bloom_slice_##SUF s; \
    s.hdr = a; \
    s.start = start; \
    s.len = (size_t)(end - start); \
    s.gen = a->gen; \
    return s; \
} \
bloom_slice_##SUF bloom_slice_##SUF##_clone(bloom_slice_##SUF s) { \
    if (s.hdr != NULL) { \
        s.hdr->rc++; \
    } \
    return s; \
} \
void bloom_slice_##SUF##_drop(bloom_slice_##SUF *s) { \
    if (s != NULL && s->hdr != NULL) { \
        bloom_arr_##SUF##_drop(&s->hdr); \
    } \
    if (s != NULL) { \
        s->hdr = NULL; \
        s->start = 0; \
        s->len = 0; \
        s->gen = 0; \
    } \
} \
T bloom_slice_##SUF##_get(bloom_slice_##SUF s, int64_t i, const char *file, int64_t line) { \
    bloom_slice_##SUF##_check(s, i, file, line, "read"); \
    return s.hdr->data[s.start + (size_t)i]; \
} \
void bloom_slice_##SUF##_set(bloom_slice_##SUF s, int64_t i, T v, const char *file, int64_t line) { \
    bloom_slice_##SUF##_check(s, i, file, line, "write"); \
    size_t idx = s.start + (size_t)i; \
    DROP_ELEM(&s.hdr->data[idx]); \
    s.hdr->data[idx] = CLONE_ELEM(v); \
} \
int64_t bloom_slice_##SUF##_len(bloom_slice_##SUF s, const char *file, int64_t line) { \
    if (s.hdr == NULL) { \
        bloom_panicf(file, line, "slice view is uninitialized (length)"); \
    } \
    if (s.hdr->gen != s.gen) { \
        bloom_panicf(file, line, "slice view was invalidated by a structural change (insert/remove/pop)"); \
    } \
    return (int64_t)s.len; \
}

BLOOM_DEFINE_SLICE_FUNCS(i64, int64_t, BLOOM_CLONE_NONE, BLOOM_DROP_NONE)
BLOOM_DEFINE_SLICE_FUNCS(f64, double, BLOOM_CLONE_NONE, BLOOM_DROP_NONE)
BLOOM_DEFINE_SLICE_FUNCS(bool, bool, BLOOM_CLONE_NONE, BLOOM_DROP_NONE)
BLOOM_DEFINE_SLICE_FUNCS(str, bloom_str, bloom_str_clone, bloom_str_drop)
BLOOM_DEFINE_SLICE_FUNCS(char, uint32_t, BLOOM_CLONE_NONE, BLOOM_DROP_NONE)

bloom_str bloom_str_join(bloom_arr_str parts, bloom_str sep) {
    size_t n = 0;
    for (size_t i = 0; i < parts->len; i++) {
        n += parts->data[i].len;
        if (i + 1 < parts->len) {
            n += sep.len;
        }
    }
    bloom_str *b = (bloom_str *)malloc(sizeof(bloom_str) + n + 1);
    if (b == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    b->rc = 1;
    b->data = (char *)(b + 1);
    b->len = n;
    size_t used = 0;
    for (size_t i = 0; i < parts->len; i++) {
        if (parts->data[i].len > 0) {
            memcpy(b->data + used, parts->data[i].data, parts->data[i].len);
            used += parts->data[i].len;
        }
        if (i + 1 < parts->len && sep.len > 0) {
            memcpy(b->data + used, sep.data, sep.len);
            used += sep.len;
        }
    }
    b->data[n] = '\0';
    bloom_live_allocs++;
    return *b;
}

bloom_arr_str bloom_str_split(bloom_str s, bloom_str sep, const char *file, int64_t line) {
    bloom_arr_str result = bloom_arr_str_new();
    if (sep.len == 0) {
        bloom_panicf(file, line, "split: separator must not be empty");
    }
    size_t start = 0;
    size_t i = 0;
    while (i < s.len) {
        if (i + sep.len <= s.len && memcmp(s.data + i, sep.data, sep.len) == 0) {
            bloom_arr_str_push_owned(result, bloom_str_from_bytes(s.data + start, i - start));
            i += sep.len;
            start = i;
        } else {
            i++;
        }
    }
    bloom_arr_str_push_owned(result, bloom_str_from_bytes(s.data + start, s.len - start));
    return result;
}
