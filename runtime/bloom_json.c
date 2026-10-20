/* Bloom std::json runtime — RFC 8259 JSON parser, value model and serializer.
 *
 * Value model: a reference-counted tree of nodes. Bloom code never sees the
 * tree directly; it holds `std::json::JsonValue` structs containing an opaque
 * handle (`Type::Json`). The handle is a *managed* Bloom value: cloning the
 * struct retains the tree, dropping it releases the tree. Accessors that
 * return sub-values (`get`, `array_get`) retain the child, so no manual
 * freeing is ever needed and error paths cannot leak.
 *
 * Numbers: parsed numbers keep their original literal text, so serializing a
 * parsed document is loss-free even for integers beyond 2^53 or floats with
 * many digits. `get_int` succeeds only for literals without fraction or
 * exponent that fit into `int` (i64); `get_float` converts through a C
 * `double`. Constructed integers serialize in decimal; constructed floats
 * use the shortest representation that round-trips.
 *
 * Object keys: insertion order is preserved. Duplicate keys are rejected by
 * the parser (no silent last-wins); `set` replaces a value in place while
 * keeping the key's position. Cycles cannot be created (`set`/`push` reject
 * them), so reference counting always reclaims the whole tree.
 *
 * Limits (all enforced with a clear error, never silently truncated):
 *   input 8388608 bytes, nesting depth 128, string 4194304 bytes,
 *   number token 128 characters, array elements 1048576, object fields
 *   1048576, total parsed values 2097152. Serialization refuses trees
 *   nested deeper than 512 levels.
 */

#define BLOOM_JSON_MAX_INPUT (8 * 1024 * 1024)
#define BLOOM_JSON_MAX_DEPTH 128
#define BLOOM_JSON_MAX_STRING (4 * 1024 * 1024)
#define BLOOM_JSON_MAX_NUMBER_TOKEN 128
#define BLOOM_JSON_MAX_ELEMENTS 1048576
#define BLOOM_JSON_MAX_VALUES 2097152
#define BLOOM_JSON_MAX_STRINGIFY_DEPTH 512

enum {
    BLOOM_JSON_NULL = 0,
    BLOOM_JSON_BOOL = 1,
    BLOOM_JSON_NUMBER = 2,
    BLOOM_JSON_STRING = 3,
    BLOOM_JSON_ARRAY = 4,
    BLOOM_JSON_OBJECT = 5
};

typedef struct bloom_json_node {
    int32_t kind;
    size_t rc;
    bool bval;
    /* number: parsed literals keep `num`; constructed values use ival/dval */
    char *num;
    size_t num_len;
    bool num_is_int;
    bool ival_ok;
    int64_t ival;
    double dval;
    /* string */
    char *str;
    size_t str_len;
    /* array: items; object: items + keys */
    struct bloom_json_node **items;
    size_t count;
    size_t cap;
    char **keys;
    size_t *key_lens;
} bloom_json_node;

typedef bloom_json_node *bloom_json_ptr;

/* ---- small helpers ------------------------------------------------------ */

static bloom_str bloom_json_msg(const char *op, const char *detail) {
    size_t opn = strlen(op);
    size_t dn = strlen(detail);
    char *buf = (char *)malloc(opn + 2 + dn + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(buf, op, opn);
    buf[opn] = ':';
    buf[opn + 1] = ' ';
    memcpy(buf + opn + 2, detail, dn);
    bloom_str s = bloom_str_from_bytes(buf, opn + 2 + dn);
    free(buf);
    return s;
}

static const char *bloom_json_kind_name(int32_t kind) {
    switch (kind) {
        case BLOOM_JSON_NULL: return "null";
        case BLOOM_JSON_BOOL: return "boolean";
        case BLOOM_JSON_NUMBER: return "number";
        case BLOOM_JSON_STRING: return "string";
        case BLOOM_JSON_ARRAY: return "array";
        default: return "object";
    }
}

/* Result/Option constructors for the concrete instantiations used here. */

static bloom_enum_Result_std_json_JsonValue_str bloom_json_ok_value(
    bloom_struct_std_json_JsonValue v
) {
    bloom_enum_Result_std_json_JsonValue_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_std_json_JsonValue_str bloom_json_err_value(bloom_str m) {
    bloom_enum_Result_std_json_JsonValue_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Option_std_json_JsonValue bloom_json_some_value(
    bloom_struct_std_json_JsonValue v
) {
    bloom_enum_Option_std_json_JsonValue r;
    r.tag = 0;
    r.data.m_Some._0 = v;
    return r;
}

static bloom_enum_Option_std_json_JsonValue bloom_json_none_value(void) {
    bloom_enum_Option_std_json_JsonValue r;
    r.tag = 1;
    return r;
}

static bloom_enum_Result_str_str bloom_json_ok_str(bloom_str v) {
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_str_str bloom_json_err_str(bloom_str m) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_i64_str bloom_json_ok_i64(int64_t v) {
    bloom_enum_Result_i64_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_i64_str bloom_json_err_i64(bloom_str m) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_f64_str bloom_json_ok_f64(double v) {
    bloom_enum_Result_f64_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_f64_str bloom_json_err_f64(bloom_str m) {
    bloom_enum_Result_f64_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_bool_str bloom_json_ok_bool(bool v) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_bool_str bloom_json_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_arr_str_str bloom_json_ok_arr(bloom_arr_str v) {
    bloom_enum_Result_arr_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_arr_str_str bloom_json_err_arr(bloom_str m) {
    bloom_enum_Result_arr_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_struct_std_json_JsonValue bloom_json_wrap(bloom_json_node *n) {
    bloom_struct_std_json_JsonValue v;
    v.f_id = (bloom_json_handle)(intptr_t)n;
    return v;
}

static bloom_json_node *bloom_json_unwrap(bloom_struct_std_json_JsonValue v) {
    return (bloom_json_node *)(intptr_t)v.f_id;
}

/* ---- node lifecycle ----------------------------------------------------- */

static bloom_json_node *bloom_json_node_new(int32_t kind) {
    bloom_json_node *n = (bloom_json_node *)calloc(1, sizeof(bloom_json_node));
    if (n == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    n->kind = kind;
    n->rc = 1;
    bloom_live_allocs++;
    return n;
}

/* Releases one reference from `root` and frees every node whose count
 * reaches zero. Shared children (referenced from several objects or held by
 * Bloom variables) stay alive. Iterative: deep trees cannot overflow the C
 * stack. */
static void bloom_json_node_release_tree(bloom_json_node *root) {
    size_t cap = 64;
    size_t count = 0;
    bloom_json_node **stack = (bloom_json_node **)malloc(cap * sizeof *stack);
    if (stack == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    stack[count++] = root;
    while (count > 0) {
        bloom_json_node *n = stack[--count];
        if (n->rc > 0) {
            n->rc--;
        }
        if (n->rc > 0) {
            continue; /* still referenced elsewhere */
        }
        if (n->kind == BLOOM_JSON_ARRAY || n->kind == BLOOM_JSON_OBJECT) {
            for (size_t i = 0; i < n->count; i++) {
                if (count == cap) {
                    cap *= 2;
                    bloom_json_node **nd = (bloom_json_node **)realloc(
                        stack,
                        cap * sizeof *stack
                    );
                    if (nd == NULL) {
                        bloom_panicf(NULL, 0, "out of memory");
                    }
                    stack = nd;
                }
                stack[count++] = n->items[i];
            }
        }
        if (n->kind == BLOOM_JSON_OBJECT) {
            for (size_t i = 0; i < n->count; i++) {
                free(n->keys[i]);
            }
            free(n->keys);
            free(n->key_lens);
        }
        free(n->items);
        free(n->num);
        free(n->str);
        free(n);
        bloom_live_allocs--;
    }
    free(stack);
}

bloom_json_handle bloom_json_retain(bloom_json_handle h) {
    bloom_json_node *n = (bloom_json_node *)(intptr_t)h;
    if (n != NULL) {
        n->rc++;
    }
    return h;
}

void bloom_json_release(bloom_json_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_json_node *n = (bloom_json_node *)(intptr_t)*h;
    *h = 0;
    bloom_json_node_release_tree(n);
}

static void bloom_json_node_append(bloom_json_node *arr, bloom_json_node *child) {
    if (arr->count == arr->cap) {
        size_t cap = arr->cap ? arr->cap * 2 : 4;
        bloom_json_node **nd = (bloom_json_node **)realloc(
            arr->items,
            cap * sizeof *nd
        );
        if (nd == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        arr->items = nd;
        arr->cap = cap;
    }
    arr->items[arr->count++] = child;
}

/* Drops one reference held by the caller (used on parse error paths). */
static void bloom_json_node_unref(bloom_json_node *n) {
    if (n == NULL) {
        return;
    }
    bloom_json_node_release_tree(n);
}

/* True if `needle` occurs inside the tree rooted at `root` (iterative). */
static bool bloom_json_tree_contains(bloom_json_node *root, bloom_json_node *needle) {
    if (root == needle) {
        return true;
    }
    size_t cap = 64;
    size_t count = 0;
    bloom_json_node **stack = (bloom_json_node **)malloc(cap * sizeof *stack);
    if (stack == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    stack[count++] = root;
    bool found = false;
    while (count > 0 && !found) {
        bloom_json_node *n = stack[--count];
        if (n == needle) {
            found = true;
            break;
        }
        if (n->kind == BLOOM_JSON_ARRAY || n->kind == BLOOM_JSON_OBJECT) {
            for (size_t i = 0; i < n->count; i++) {
                if (count == cap) {
                    cap *= 2;
                    bloom_json_node **nd = (bloom_json_node **)realloc(
                        stack,
                        cap * sizeof *stack
                    );
                    if (nd == NULL) {
                        bloom_panicf(NULL, 0, "out of memory");
                    }
                    stack = nd;
                }
                stack[count++] = n->items[i];
            }
        }
    }
    free(stack);
    return found;
}

/* ---- parser ------------------------------------------------------------- */

typedef struct {
    const char *p;
    size_t len;
    size_t pos;
    int depth;
    int64_t values;
    const char *fail;
    size_t fail_pos;
} bloom_json_parser;

static bloom_json_node *bloom_json_parse_value(bloom_json_parser *ps);

static void bloom_json_fail(bloom_json_parser *ps, const char *reason) {
    if (ps->fail == NULL) {
        ps->fail = reason;
        ps->fail_pos = ps->pos;
    }
}

static void bloom_json_skip_ws(bloom_json_parser *ps) {
    while (ps->pos < ps->len) {
        char c = ps->p[ps->pos];
        if (c == ' ' || c == '\t' || c == '\n' || c == '\r') {
            ps->pos++;
        } else {
            break;
        }
    }
}

static bloom_str bloom_json_fail_message(bloom_json_parser *ps) {
    size_t line = 1;
    size_t col = 1;
    size_t stop = ps->fail_pos < ps->len ? ps->fail_pos : ps->len;
    for (size_t i = 0; i < stop; i++) {
        if (ps->p[i] == '\n') {
            line++;
            col = 1;
        } else {
            col++;
        }
    }
    char posbuf[96];
    snprintf(
        posbuf,
        sizeof posbuf,
        " at byte %zu (line %zu, column %zu)",
        stop + 1,
        line,
        col
    );
    size_t rn = strlen(ps->fail);
    size_t pn = strlen(posbuf);
    char *buf = (char *)malloc(rn + pn + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(buf, ps->fail, rn);
    memcpy(buf + rn, posbuf, pn);
    bloom_str msg = bloom_str_from_bytes(buf, rn + pn);
    free(buf);
    return msg;
}

static int bloom_json_hex_digit(char c) {
    if (c >= '0' && c <= '9') {
        return c - '0';
    }
    if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
    }
    if (c >= 'A' && c <= 'F') {
        return c - 'A' + 10;
    }
    return -1;
}

static bool bloom_json_parse_hex4(bloom_json_parser *ps, uint32_t *out) {
    if (ps->pos + 4 > ps->len) {
        bloom_json_fail(ps, "truncated `\\u` escape");
        return false;
    }
    uint32_t v = 0;
    for (int i = 0; i < 4; i++) {
        int d = bloom_json_hex_digit(ps->p[ps->pos + (size_t)i]);
        if (d < 0) {
            ps->pos += (size_t)i;
            bloom_json_fail(ps, "invalid `\\u` escape (expected four hex digits)");
            return false;
        }
        v = v * 16 + (uint32_t)d;
    }
    ps->pos += 4;
    *out = v;
    return true;
}

static size_t bloom_json_utf8_len(uint32_t cp) {
    if (cp < 0x80u) {
        return 1;
    }
    if (cp < 0x800u) {
        return 2;
    }
    if (cp < 0x10000u) {
        return 3;
    }
    return 4;
}

/* Parses a JSON string (after the opening quote) into fresh bytes. */
static bool bloom_json_parse_string(
    bloom_json_parser *ps,
    char **out,
    size_t *out_len
) {
    size_t cap = 32;
    size_t at = 0;
    char *buf = (char *)malloc(cap);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    for (;;) {
        if (ps->pos >= ps->len) {
            free(buf);
            bloom_json_fail(ps, "unterminated string");
            return false;
        }
        unsigned char c = (unsigned char)ps->p[ps->pos];
        if (c == '"') {
            ps->pos++;
            break;
        }
        uint32_t cp = 0;
        bool escaped = false;
        if (c == '\\') {
            escaped = true;
            ps->pos++;
            if (ps->pos >= ps->len) {
                free(buf);
                bloom_json_fail(ps, "unterminated escape sequence");
                return false;
            }
            char e = ps->p[ps->pos++];
            switch (e) {
                case '"': cp = '"'; break;
                case '\\': cp = '\\'; break;
                case '/': cp = '/'; break;
                case 'b': cp = '\b'; break;
                case 'f': cp = '\f'; break;
                case 'n': cp = '\n'; break;
                case 'r': cp = '\r'; break;
                case 't': cp = '\t'; break;
                case 'u': {
                    uint32_t hi = 0;
                    if (!bloom_json_parse_hex4(ps, &hi)) {
                        free(buf);
                        return false;
                    }
                    if (hi >= 0xD800 && hi <= 0xDBFF) {
                        if (ps->pos + 2 > ps->len || ps->p[ps->pos] != '\\'
                            || ps->p[ps->pos + 1] != 'u') {
                            free(buf);
                            bloom_json_fail(ps, "high surrogate without a following low surrogate");
                            return false;
                        }
                        ps->pos += 2;
                        uint32_t lo = 0;
                        if (!bloom_json_parse_hex4(ps, &lo)) {
                            free(buf);
                            return false;
                        }
                        if (lo < 0xDC00 || lo > 0xDFFF) {
                            free(buf);
                            bloom_json_fail(ps, "high surrogate without a following low surrogate");
                            return false;
                        }
                        cp = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                    } else if (hi >= 0xDC00 && hi <= 0xDFFF) {
                        free(buf);
                        bloom_json_fail(ps, "lone low surrogate in `\\u` escape");
                        return false;
                    } else {
                        cp = hi;
                    }
                    break;
                }
                default:
                    free(buf);
                    ps->pos--; /* point at the invalid escape character */
                    bloom_json_fail(ps, "invalid escape sequence");
                    return false;
            }
        } else if (c < 0x20) {
            free(buf);
            bloom_json_fail(ps, "unescaped control character in string");
            return false;
        } else {
            /* Raw input bytes (the input is valid UTF-8) are copied
             * verbatim; only escape sequences produce code points. */
            if (at + 1 > BLOOM_JSON_MAX_STRING) {
                free(buf);
                bloom_json_fail(ps, "string too long (max 4194304 bytes)");
                return false;
            }
            if (at + 1 > cap) {
                while (cap < at + 1) {
                    cap *= 2;
                }
                char *nd = (char *)realloc(buf, cap);
                if (nd == NULL) {
                    bloom_panicf(NULL, 0, "out of memory");
                }
                buf = nd;
            }
            buf[at++] = (char)c;
            ps->pos++;
            continue;
        }
        size_t used = bloom_json_utf8_len(cp);
        if (at + used > BLOOM_JSON_MAX_STRING) {
            free(buf);
            bloom_json_fail(ps, "string too long (max 4194304 bytes)");
            return false;
        }
        if (at + used > cap) {
            while (cap < at + used) {
                cap *= 2;
            }
            char *nd = (char *)realloc(buf, cap);
            if (nd == NULL) {
                bloom_panicf(NULL, 0, "out of memory");
            }
            buf = nd;
        }
        if (escaped) {
            bloom_utf8_encode(cp, buf + at);
        }
        at += used;
    }
    *out = buf;
    *out_len = at;
    return true;
}

static bool bloom_json_parse_int64(
    const char *p,
    size_t len,
    int64_t *out
) {
    size_t i = 0;
    bool neg = false;
    if (i < len && p[i] == '-') {
        neg = true;
        i++;
    }
    if (i >= len) {
        return false;
    }
    uint64_t acc = 0;
    uint64_t limit = neg ? (uint64_t)INT64_MAX + 1 : (uint64_t)INT64_MAX;
    for (; i < len; i++) {
        if (p[i] < '0' || p[i] > '9') {
            return false;
        }
        uint64_t d = (uint64_t)(p[i] - '0');
        if (acc > (limit - d) / 10) {
            return false;
        }
        acc = acc * 10 + d;
    }
    if (neg) {
        *out = acc == (uint64_t)INT64_MAX + 1 ? INT64_MIN : -(int64_t)acc;
    } else {
        *out = (int64_t)acc;
    }
    return true;
}

static bloom_json_node *bloom_json_parse_number(bloom_json_parser *ps) {
    size_t start = ps->pos;
    size_t i = ps->pos;
    const char *p = ps->p;
    size_t len = ps->len;
    if (i < len && p[i] == '-') {
        i++;
    }
    if (i >= len || p[i] < '0' || p[i] > '9') {
        ps->pos = i;
        bloom_json_fail(ps, "invalid number");
        return NULL;
    }
    if (p[i] == '0') {
        i++;
        if (i < len && p[i] >= '0' && p[i] <= '9') {
            ps->pos = i;
            bloom_json_fail(ps, "invalid number (leading zeros are not allowed)");
            return NULL;
        }
    } else {
        while (i < len && p[i] >= '0' && p[i] <= '9') {
            i++;
        }
    }
    bool integral = true;
    if (i < len && p[i] == '.') {
        integral = false;
        i++;
        if (i >= len || p[i] < '0' || p[i] > '9') {
            ps->pos = i;
            bloom_json_fail(ps, "invalid number (missing digits after `.`)");
            return NULL;
        }
        while (i < len && p[i] >= '0' && p[i] <= '9') {
            i++;
        }
    }
    if (i < len && (p[i] == 'e' || p[i] == 'E')) {
        integral = false;
        i++;
        if (i < len && (p[i] == '+' || p[i] == '-')) {
            i++;
        }
        if (i >= len || p[i] < '0' || p[i] > '9') {
            ps->pos = i;
            bloom_json_fail(ps, "invalid number (missing exponent digits)");
            return NULL;
        }
        while (i < len && p[i] >= '0' && p[i] <= '9') {
            i++;
        }
    }
    size_t tok_len = i - start;
    if (tok_len > BLOOM_JSON_MAX_NUMBER_TOKEN) {
        ps->pos = start;
        bloom_json_fail(ps, "number token too long (max 128 characters)");
        return NULL;
    }
    ps->pos = i;
    bloom_json_node *n = bloom_json_node_new(BLOOM_JSON_NUMBER);
    n->num = (char *)malloc(tok_len + 1);
    if (n->num == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(n->num, p + start, tok_len);
    n->num[tok_len] = '\0';
    n->num_len = tok_len;
    n->num_is_int = integral;
    if (integral) {
        n->ival_ok = bloom_json_parse_int64(n->num, tok_len, &n->ival);
    }
    return n;
}

static bloom_json_node *bloom_json_parse_literal(
    bloom_json_parser *ps,
    const char *text,
    int32_t kind,
    bool bval
) {
    size_t n = strlen(text);
    if (ps->pos + n > ps->len || memcmp(ps->p + ps->pos, text, n) != 0) {
        bloom_json_fail(ps, "invalid literal");
        return NULL;
    }
    ps->pos += n;
    bloom_json_node *node = bloom_json_node_new(kind);
    node->bval = bval;
    return node;
}

static bloom_json_node *bloom_json_parse_array(bloom_json_parser *ps) {
    bloom_json_node *arr = bloom_json_node_new(BLOOM_JSON_ARRAY);
    ps->pos++; /* '[' */
    bloom_json_skip_ws(ps);
    if (ps->pos < ps->len && ps->p[ps->pos] == ']') {
        ps->pos++;
        return arr;
    }
    for (;;) {
        if (arr->count >= BLOOM_JSON_MAX_ELEMENTS) {
            bloom_json_fail(ps, "too many array elements (max 1048576)");
            bloom_json_node_unref(arr);
            return NULL;
        }
        bloom_json_node *child = bloom_json_parse_value(ps);
        if (child == NULL) {
            bloom_json_node_unref(arr);
            return NULL;
        }
        bloom_json_node_append(arr, child);
        bloom_json_skip_ws(ps);
        if (ps->pos >= ps->len) {
            bloom_json_fail(ps, "unterminated array");
            bloom_json_node_unref(arr);
            return NULL;
        }
        char c = ps->p[ps->pos];
        if (c == ',') {
            ps->pos++;
            bloom_json_skip_ws(ps);
            continue;
        }
        if (c == ']') {
            ps->pos++;
            return arr;
        }
        bloom_json_fail(ps, "expected `,` or `]` in array");
        bloom_json_node_unref(arr);
        return NULL;
    }
}

typedef struct {
    const char *p;
    size_t len;
} bloom_json_sortkey;

static int bloom_json_sortkey_cmp(const void *a, const void *b) {
    const bloom_json_sortkey *ka = (const bloom_json_sortkey *)a;
    const bloom_json_sortkey *kb = (const bloom_json_sortkey *)b;
    size_t n = ka->len < kb->len ? ka->len : kb->len;
    int c = n > 0 ? memcmp(ka->p, kb->p, n) : 0;
    if (c != 0) {
        return c;
    }
    if (ka->len < kb->len) {
        return -1;
    }
    if (ka->len > kb->len) {
        return 1;
    }
    return 0;
}

/* Detects duplicate keys with an index sort (O(n log n)). */
static bool bloom_json_has_duplicate_keys(bloom_json_node *obj) {
    size_t n = obj->count;
    if (n < 2) {
        return false;
    }
    bloom_json_sortkey *keys = (bloom_json_sortkey *)malloc(n * sizeof *keys);
    if (keys == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    for (size_t i = 0; i < n; i++) {
        keys[i].p = obj->keys[i];
        keys[i].len = obj->key_lens[i];
    }
    qsort(keys, n, sizeof *keys, bloom_json_sortkey_cmp);
    bool dup = false;
    for (size_t i = 1; i < n && !dup; i++) {
        if (keys[i].len == keys[i - 1].len
            && memcmp(keys[i].p, keys[i - 1].p, keys[i].len) == 0) {
            dup = true;
        }
    }
    free(keys);
    return dup;
}

static bloom_json_node *bloom_json_parse_object(bloom_json_parser *ps) {
    bloom_json_node *obj = bloom_json_node_new(BLOOM_JSON_OBJECT);
    ps->pos++; /* '{' */
    bloom_json_skip_ws(ps);
    if (ps->pos < ps->len && ps->p[ps->pos] == '}') {
        ps->pos++;
        return obj;
    }
    for (;;) {
        if (obj->count >= BLOOM_JSON_MAX_ELEMENTS) {
            bloom_json_fail(ps, "too many object fields (max 1048576)");
            bloom_json_node_unref(obj);
            return NULL;
        }
        bloom_json_skip_ws(ps);
        if (ps->pos >= ps->len || ps->p[ps->pos] != '"') {
            bloom_json_fail(ps, "expected a string key");
            bloom_json_node_unref(obj);
            return NULL;
        }
        ps->pos++;
        char *key = NULL;
        size_t key_len = 0;
        if (!bloom_json_parse_string(ps, &key, &key_len)) {
            bloom_json_node_unref(obj);
            return NULL;
        }
        bloom_json_skip_ws(ps);
        if (ps->pos >= ps->len || ps->p[ps->pos] != ':') {
            free(key);
            bloom_json_fail(ps, "expected `:` after object key");
            bloom_json_node_unref(obj);
            return NULL;
        }
        ps->pos++;
        bloom_json_skip_ws(ps);
        bloom_json_node *child = bloom_json_parse_value(ps);
        if (child == NULL) {
            free(key);
            bloom_json_node_unref(obj);
            return NULL;
        }
        /* Grow the key arrays in lockstep with items. */
        if (obj->count == obj->cap) {
            size_t cap = obj->cap ? obj->cap * 2 : 4;
            bloom_json_node **nd = (bloom_json_node **)realloc(
                obj->items,
                cap * sizeof *nd
            );
            if (nd == NULL) {
                bloom_panicf(NULL, 0, "out of memory");
            }
            obj->items = nd;
            char **nk = (char **)realloc(obj->keys, cap * sizeof *nk);
            if (nk == NULL) {
                bloom_panicf(NULL, 0, "out of memory");
            }
            obj->keys = nk;
            size_t *nl = (size_t *)realloc(obj->key_lens, cap * sizeof *nl);
            if (nl == NULL) {
                bloom_panicf(NULL, 0, "out of memory");
            }
            obj->key_lens = nl;
            obj->cap = cap;
        }
        obj->keys[obj->count] = key;
        obj->key_lens[obj->count] = key_len;
        obj->items[obj->count] = child;
        obj->count++;
        bloom_json_skip_ws(ps);
        if (ps->pos >= ps->len) {
            bloom_json_fail(ps, "unterminated object");
            bloom_json_node_unref(obj);
            return NULL;
        }
        char c = ps->p[ps->pos];
        if (c == ',') {
            ps->pos++;
            continue;
        }
        if (c == '}') {
            ps->pos++;
            if (bloom_json_has_duplicate_keys(obj)) {
                bloom_json_fail(ps, "duplicate object key");
                bloom_json_node_unref(obj);
                return NULL;
            }
            return obj;
        }
        bloom_json_fail(ps, "expected `,` or `}` in object");
        bloom_json_node_unref(obj);
        return NULL;
    }
}

static bloom_json_node *bloom_json_parse_value(bloom_json_parser *ps) {
    if (ps->depth >= BLOOM_JSON_MAX_DEPTH) {
        bloom_json_fail(ps, "nesting too deep (max 128)");
        return NULL;
    }
    if (ps->values >= BLOOM_JSON_MAX_VALUES) {
        bloom_json_fail(ps, "too many values (max 2097152)");
        return NULL;
    }
    bloom_json_skip_ws(ps);
    if (ps->pos >= ps->len) {
        bloom_json_fail(ps, "unexpected end of input");
        return NULL;
    }
    ps->values++;
    char c = ps->p[ps->pos];
    bloom_json_node *result = NULL;
    if (c == '{') {
        ps->depth++;
        result = bloom_json_parse_object(ps);
        ps->depth--;
    } else if (c == '[') {
        ps->depth++;
        result = bloom_json_parse_array(ps);
        ps->depth--;
    } else if (c == '"') {
        ps->pos++;
        char *s = NULL;
        size_t s_len = 0;
        if (bloom_json_parse_string(ps, &s, &s_len)) {
            result = bloom_json_node_new(BLOOM_JSON_STRING);
            result->str = s;
            result->str_len = s_len;
        }
    } else if (c == 't') {
        result = bloom_json_parse_literal(ps, "true", BLOOM_JSON_BOOL, true);
    } else if (c == 'f') {
        result = bloom_json_parse_literal(ps, "false", BLOOM_JSON_BOOL, false);
    } else if (c == 'n') {
        result = bloom_json_parse_literal(ps, "null", BLOOM_JSON_NULL, false);
    } else if (c == '-' || (c >= '0' && c <= '9')) {
        result = bloom_json_parse_number(ps);
    } else {
        bloom_json_fail(ps, "unexpected character");
    }
    return result;
}

/* ---- serializer --------------------------------------------------------- */

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} bloom_json_buf;

static void bloom_json_buf_init(bloom_json_buf *b) {
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
}

static void bloom_json_buf_push(bloom_json_buf *b, const char *p, size_t n) {
    if (n == 0) {
        return;
    }
    if (b->len + n + 1 > b->cap) {
        size_t cap = b->cap ? b->cap : 64;
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

/* Shortest `%g` representation that round-trips through `strtod`. */
static void bloom_json_format_double(double v, char *buf, size_t bufsize) {
    for (int prec = 15; prec <= 17; prec++) {
        snprintf(buf, bufsize, "%.*g", prec, v);
        if (strtod(buf, NULL) == v) {
            return;
        }
    }
}

static bool bloom_json_emit(
    bloom_json_node *n,
    bloom_json_buf *out,
    int depth,
    bool pretty,
    int indent,
    bloom_str *err
) {
    if (depth > BLOOM_JSON_MAX_STRINGIFY_DEPTH) {
        *err = bloom_json_msg("stringify", "structure too deeply nested (max 512)");
        return false;
    }
    switch (n->kind) {
        case BLOOM_JSON_NULL:
            bloom_json_buf_push(out, "null", 4);
            return true;
        case BLOOM_JSON_BOOL:
            if (n->bval) {
                bloom_json_buf_push(out, "true", 4);
            } else {
                bloom_json_buf_push(out, "false", 5);
            }
            return true;
        case BLOOM_JSON_NUMBER: {
            if (n->num != NULL) {
                bloom_json_buf_push(out, n->num, n->num_len);
                return true;
            }
            char tmp[64];
            if (n->num_is_int) {
                snprintf(tmp, sizeof tmp, "%" PRId64, n->ival);
            } else {
                if (!isfinite(n->dval)) {
                    *err = bloom_json_msg(
                        "stringify",
                        "cannot serialize a non-finite number"
                    );
                    return false;
                }
                bloom_json_format_double(n->dval, tmp, sizeof tmp);
            }
            bloom_json_buf_push(out, tmp, strlen(tmp));
            return true;
        }
        case BLOOM_JSON_STRING: {
            bloom_json_buf_push(out, "\"", 1);
            for (size_t i = 0; i < n->str_len; i++) {
                unsigned char c = (unsigned char)n->str[i];
                switch (c) {
                    case '"': bloom_json_buf_push(out, "\\\"", 2); break;
                    case '\\': bloom_json_buf_push(out, "\\\\", 2); break;
                    case '\b': bloom_json_buf_push(out, "\\b", 2); break;
                    case '\f': bloom_json_buf_push(out, "\\f", 2); break;
                    case '\n': bloom_json_buf_push(out, "\\n", 2); break;
                    case '\r': bloom_json_buf_push(out, "\\r", 2); break;
                    case '\t': bloom_json_buf_push(out, "\\t", 2); break;
                    default:
                        if (c < 0x20) {
                            char esc[8];
                            snprintf(esc, sizeof esc, "\\u%04x", c);
                            bloom_json_buf_push(out, esc, 6);
                        } else {
                            bloom_json_buf_push(out, (const char *)&n->str[i], 1);
                        }
                        break;
                }
            }
            bloom_json_buf_push(out, "\"", 1);
            return true;
        }
        case BLOOM_JSON_ARRAY: {
            bloom_json_buf_push(out, "[", 1);
            for (size_t i = 0; i < n->count; i++) {
                if (i > 0) {
                    bloom_json_buf_push(out, ",", 1);
                }
                if (pretty) {
                    bloom_json_buf_push(out, "\n", 1);
                    for (int k = 0; k < indent + 1; k++) {
                        bloom_json_buf_push(out, "  ", 2);
                    }
                }
                if (!bloom_json_emit(n->items[i], out, depth + 1, pretty, indent + 1, err)) {
                    return false;
                }
            }
            if (pretty && n->count > 0) {
                bloom_json_buf_push(out, "\n", 1);
                for (int k = 0; k < indent; k++) {
                    bloom_json_buf_push(out, "  ", 2);
                }
            }
            bloom_json_buf_push(out, "]", 1);
            return true;
        }
        default: { /* object */
            bloom_json_buf_push(out, "{", 1);
            for (size_t i = 0; i < n->count; i++) {
                if (i > 0) {
                    bloom_json_buf_push(out, ",", 1);
                }
                if (pretty) {
                    bloom_json_buf_push(out, "\n", 1);
                    for (int k = 0; k < indent + 1; k++) {
                        bloom_json_buf_push(out, "  ", 2);
                    }
                }
                bloom_json_node key_node;
                memset(&key_node, 0, sizeof key_node);
                key_node.kind = BLOOM_JSON_STRING;
                key_node.str = n->keys[i];
                key_node.str_len = n->key_lens[i];
                if (!bloom_json_emit(&key_node, out, depth + 1, pretty, indent + 1, err)) {
                    return false;
                }
                bloom_json_buf_push(out, pretty ? ": " : ":", pretty ? 2 : 1);
                if (!bloom_json_emit(n->items[i], out, depth + 1, pretty, indent + 1, err)) {
                    return false;
                }
            }
            if (pretty && n->count > 0) {
                bloom_json_buf_push(out, "\n", 1);
                for (int k = 0; k < indent; k++) {
                    bloom_json_buf_push(out, "  ", 2);
                }
            }
            bloom_json_buf_push(out, "}", 1);
            return true;
        }
    }
}

/* ---- field lookup ------------------------------------------------------- */

static bloom_json_node *bloom_json_object_find(
    bloom_json_node *obj,
    const char *key,
    size_t key_len
) {
    for (size_t i = 0; i < obj->count; i++) {
        if (obj->key_lens[i] == key_len
            && memcmp(obj->keys[i], key, key_len) == 0) {
            return obj->items[i];
        }
    }
    return NULL;
}

/* ---- public API --------------------------------------------------------- */

bloom_enum_Result_std_json_JsonValue_str bloom_json_parse(bloom_str input) {
    if (input.len > BLOOM_JSON_MAX_INPUT) {
        return bloom_json_err_value(bloom_json_msg(
            "parse",
            "input too large (max 8388608 bytes)"
        ));
    }
    bloom_json_parser ps;
    ps.p = input.data != NULL ? input.data : "";
    ps.len = input.len;
    ps.pos = 0;
    ps.depth = 0;
    ps.values = 0;
    ps.fail = NULL;
    ps.fail_pos = 0;
    bloom_json_node *root = bloom_json_parse_value(&ps);
    if (root == NULL) {
        return bloom_json_err_value(bloom_json_fail_message(&ps));
    }
    bloom_json_skip_ws(&ps);
    if (ps.pos != ps.len) {
        ps.fail = "trailing characters after the JSON value";
        ps.fail_pos = ps.pos;
        bloom_json_node_unref(root);
        return bloom_json_err_value(bloom_json_fail_message(&ps));
    }
    return bloom_json_ok_value(bloom_json_wrap(root));
}

bloom_enum_Result_str_str bloom_json_stringify(bloom_struct_std_json_JsonValue v) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL) {
        return bloom_json_err_str(bloom_json_msg("stringify", "invalid JSON value"));
    }
    bloom_json_buf out;
    bloom_json_buf_init(&out);
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    if (!bloom_json_emit(n, &out, 0, false, 0, &err)) {
        free(out.data);
        return bloom_json_err_str(err);
    }
    bloom_str result = bloom_str_from_bytes(out.data != NULL ? out.data : "", out.len);
    free(out.data);
    return bloom_json_ok_str(result);
}

bloom_enum_Result_str_str bloom_json_stringify_pretty(bloom_struct_std_json_JsonValue v) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL) {
        return bloom_json_err_str(bloom_json_msg("stringify_pretty", "invalid JSON value"));
    }
    bloom_json_buf out;
    bloom_json_buf_init(&out);
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    if (!bloom_json_emit(n, &out, 0, true, 0, &err)) {
        free(out.data);
        return bloom_json_err_str(err);
    }
    bloom_str result = bloom_str_from_bytes(out.data != NULL ? out.data : "", out.len);
    free(out.data);
    return bloom_json_ok_str(result);
}

bloom_enum_Option_std_json_JsonValue bloom_json_get(
    bloom_struct_std_json_JsonValue v,
    bloom_str key
) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL || n->kind != BLOOM_JSON_OBJECT) {
        return bloom_json_none_value();
    }
    bloom_json_node *child = bloom_json_object_find(n, key.data, key.len);
    if (child == NULL) {
        return bloom_json_none_value();
    }
    child->rc++;
    return bloom_json_some_value(bloom_json_wrap(child));
}

bool bloom_json_has(bloom_struct_std_json_JsonValue v, bloom_str key) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL || n->kind != BLOOM_JSON_OBJECT) {
        return false;
    }
    return bloom_json_object_find(n, key.data, key.len) != NULL;
}

static bloom_json_node *bloom_json_get_checked(
    bloom_struct_std_json_JsonValue v,
    bloom_str key,
    const char *op,
    bloom_str *err
) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL) {
        *err = bloom_json_msg(op, "invalid JSON value");
        return NULL;
    }
    if (n->kind != BLOOM_JSON_OBJECT) {
        size_t dn = strlen(op) + 40;
        char *buf = (char *)malloc(dn);
        if (buf == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        snprintf(buf, dn, "%s: value is not an object (it is %s)", op, bloom_json_kind_name(n->kind));
        *err = bloom_str_from_bytes(buf, strlen(buf));
        free(buf);
        return NULL;
    }
    bloom_json_node *child = bloom_json_object_find(n, key.data, key.len);
    if (child == NULL) {
        size_t dn = strlen(op) + key.len + 32;
        char *buf = (char *)malloc(dn);
        if (buf == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        int written = snprintf(buf, dn, "%s: key `%.*s` not found", op, (int)key.len, key.data);
        *err = bloom_str_from_bytes(buf, (size_t)written);
        free(buf);
        return NULL;
    }
    return child;
}

static bloom_str bloom_json_type_err(const char *op, bloom_str key, int32_t kind) {
    size_t dn = strlen(op) + key.len + 64;
    char *buf = (char *)malloc(dn);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    int written = snprintf(
        buf,
        dn,
        "%s: value for key `%.*s` is %s",
        op,
        (int)key.len,
        key.data,
        bloom_json_kind_name(kind)
    );
    bloom_str s = bloom_str_from_bytes(buf, (size_t)written);
    free(buf);
    return s;
}

bloom_enum_Result_str_str bloom_json_get_string(
    bloom_struct_std_json_JsonValue v,
    bloom_str key
) {
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    bloom_json_node *n = bloom_json_get_checked(v, key, "get_string", &err);
    if (n == NULL) {
        return bloom_json_err_str(err);
    }
    if (n->kind != BLOOM_JSON_STRING) {
        return bloom_json_err_str(bloom_json_type_err("get_string", key, n->kind));
    }
    return bloom_json_ok_str(bloom_str_from_bytes(n->str, n->str_len));
}

bloom_enum_Result_i64_str bloom_json_get_int(
    bloom_struct_std_json_JsonValue v,
    bloom_str key
) {
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    bloom_json_node *n = bloom_json_get_checked(v, key, "get_int", &err);
    if (n == NULL) {
        return bloom_json_err_i64(err);
    }
    if (n->kind != BLOOM_JSON_NUMBER) {
        return bloom_json_err_i64(bloom_json_type_err("get_int", key, n->kind));
    }
    if (n->num != NULL && !n->num_is_int) {
        return bloom_json_err_i64(bloom_json_msg(
            "get_int",
            "number has a fraction or exponent (use get_float)"
        ));
    }
    if (n->num != NULL && !n->ival_ok) {
        return bloom_json_err_i64(bloom_json_msg(
            "get_int",
            "integer out of range for `int`"
        ));
    }
    if (n->num == NULL && !n->num_is_int) {
        return bloom_json_err_i64(bloom_json_msg(
            "get_int",
            "number is a float (use get_float)"
        ));
    }
    return bloom_json_ok_i64(n->ival);
}

bloom_enum_Result_f64_str bloom_json_get_float(
    bloom_struct_std_json_JsonValue v,
    bloom_str key
) {
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    bloom_json_node *n = bloom_json_get_checked(v, key, "get_float", &err);
    if (n == NULL) {
        return bloom_json_err_f64(err);
    }
    if (n->kind != BLOOM_JSON_NUMBER) {
        return bloom_json_err_f64(bloom_json_type_err("get_float", key, n->kind));
    }
    if (n->num != NULL) {
        return bloom_json_ok_f64(strtod(n->num, NULL));
    }
    if (n->num_is_int) {
        return bloom_json_ok_f64((double)n->ival);
    }
    return bloom_json_ok_f64(n->dval);
}

bloom_enum_Result_bool_str bloom_json_get_bool(
    bloom_struct_std_json_JsonValue v,
    bloom_str key
) {
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    bloom_json_node *n = bloom_json_get_checked(v, key, "get_bool", &err);
    if (n == NULL) {
        return bloom_json_err_bool(err);
    }
    if (n->kind != BLOOM_JSON_BOOL) {
        return bloom_json_err_bool(bloom_json_type_err("get_bool", key, n->kind));
    }
    return bloom_json_ok_bool(n->bval);
}

static bloom_str bloom_json_value_type_err(const char *op, int32_t kind) {
    size_t dn = strlen(op) + 64;
    char *buf = (char *)malloc(dn);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    int written = snprintf(buf, dn, "%s: value is %s", op, bloom_json_kind_name(kind));
    bloom_str s = bloom_str_from_bytes(buf, (size_t)written);
    free(buf);
    return s;
}

bloom_enum_Result_str_str bloom_json_as_string(bloom_struct_std_json_JsonValue v) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL) {
        return bloom_json_err_str(bloom_json_msg("as_string", "invalid JSON value"));
    }
    if (n->kind != BLOOM_JSON_STRING) {
        return bloom_json_err_str(bloom_json_value_type_err("as_string", n->kind));
    }
    return bloom_json_ok_str(bloom_str_from_bytes(n->str, n->str_len));
}

bloom_enum_Result_i64_str bloom_json_as_int(bloom_struct_std_json_JsonValue v) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL) {
        return bloom_json_err_i64(bloom_json_msg("as_int", "invalid JSON value"));
    }
    if (n->kind != BLOOM_JSON_NUMBER) {
        return bloom_json_err_i64(bloom_json_value_type_err("as_int", n->kind));
    }
    if (n->num != NULL && !n->num_is_int) {
        return bloom_json_err_i64(bloom_json_msg(
            "as_int",
            "number has a fraction or exponent (use as_float)"
        ));
    }
    if (n->num != NULL && !n->ival_ok) {
        return bloom_json_err_i64(bloom_json_msg(
            "as_int",
            "integer out of range for `int`"
        ));
    }
    if (n->num == NULL && !n->num_is_int) {
        return bloom_json_err_i64(bloom_json_msg(
            "as_int",
            "number is a float (use as_float)"
        ));
    }
    return bloom_json_ok_i64(n->ival);
}

bloom_enum_Result_f64_str bloom_json_as_float(bloom_struct_std_json_JsonValue v) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL) {
        return bloom_json_err_f64(bloom_json_msg("as_float", "invalid JSON value"));
    }
    if (n->kind != BLOOM_JSON_NUMBER) {
        return bloom_json_err_f64(bloom_json_value_type_err("as_float", n->kind));
    }
    if (n->num != NULL) {
        return bloom_json_ok_f64(strtod(n->num, NULL));
    }
    if (n->num_is_int) {
        return bloom_json_ok_f64((double)n->ival);
    }
    return bloom_json_ok_f64(n->dval);
}

bloom_enum_Result_bool_str bloom_json_as_bool(bloom_struct_std_json_JsonValue v) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL) {
        return bloom_json_err_bool(bloom_json_msg("as_bool", "invalid JSON value"));
    }
    if (n->kind != BLOOM_JSON_BOOL) {
        return bloom_json_err_bool(bloom_json_value_type_err("as_bool", n->kind));
    }
    return bloom_json_ok_bool(n->bval);
}

bloom_enum_Result_i64_str bloom_json_array_length(bloom_struct_std_json_JsonValue v) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL) {
        return bloom_json_err_i64(bloom_json_msg("array_length", "invalid JSON value"));
    }
    if (n->kind != BLOOM_JSON_ARRAY) {
        size_t dn = 80;
        char *buf = (char *)malloc(dn);
        if (buf == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        snprintf(
            buf,
            dn,
            "array_length: value is not an array (it is %s)",
            bloom_json_kind_name(n->kind)
        );
        bloom_str err = bloom_str_from_bytes(buf, strlen(buf));
        free(buf);
        return bloom_json_err_i64(err);
    }
    return bloom_json_ok_i64((int64_t)n->count);
}

bloom_enum_Option_std_json_JsonValue bloom_json_array_get(
    bloom_struct_std_json_JsonValue v,
    int64_t index
) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL || n->kind != BLOOM_JSON_ARRAY) {
        return bloom_json_none_value();
    }
    if (index < 0 || (uint64_t)index >= (uint64_t)n->count) {
        return bloom_json_none_value();
    }
    bloom_json_node *child = n->items[index];
    child->rc++;
    return bloom_json_some_value(bloom_json_wrap(child));
}

bloom_enum_Result_arr_str_str bloom_json_object_keys(bloom_struct_std_json_JsonValue v) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL) {
        return bloom_json_err_arr(bloom_json_msg("object_keys", "invalid JSON value"));
    }
    if (n->kind != BLOOM_JSON_OBJECT) {
        size_t dn = 80;
        char *buf = (char *)malloc(dn);
        if (buf == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        snprintf(
            buf,
            dn,
            "object_keys: value is not an object (it is %s)",
            bloom_json_kind_name(n->kind)
        );
        bloom_str err = bloom_str_from_bytes(buf, strlen(buf));
        free(buf);
        return bloom_json_err_arr(err);
    }
    bloom_arr_str out = bloom_arr_str_new();
    for (size_t i = 0; i < n->count; i++) {
        bloom_arr_str_push_owned(
            out,
            bloom_str_from_bytes(n->keys[i], n->key_lens[i])
        );
    }
    return bloom_json_ok_arr(out);
}

bloom_enum_Result_bool_str bloom_json_set(
    bloom_struct_std_json_JsonValue v,
    bloom_str key,
    bloom_struct_std_json_JsonValue item
) {
    bloom_json_node *n = bloom_json_unwrap(v);
    bloom_json_node *child = bloom_json_unwrap(item);
    if (n == NULL || child == NULL) {
        return bloom_json_err_bool(bloom_json_msg("set", "invalid JSON value"));
    }
    if (n->kind != BLOOM_JSON_OBJECT) {
        return bloom_json_err_bool(bloom_json_msg(
            "set",
            "value is not an object (create one with make_object)"
        ));
    }
    if (bloom_json_tree_contains(child, n)) {
        return bloom_json_err_bool(bloom_json_msg(
            "set",
            "this would create a cycle (the value already contains the object)"
        ));
    }
    bloom_json_node *existing = bloom_json_object_find(n, key.data, key.len);
    if (existing != NULL) {
        /* Replace in place; the key keeps its position. */
        child->rc++;
        for (size_t i = 0; i < n->count; i++) {
            if (n->items[i] == existing) {
                n->items[i] = child;
                break;
            }
        }
        /* Release the replaced value's reference; shared nodes stay alive. */
        bloom_json_node_release_tree(existing);
        return bloom_json_ok_bool(true);
    }
    if (n->count >= BLOOM_JSON_MAX_ELEMENTS) {
        return bloom_json_err_bool(bloom_json_msg(
            "set",
            "too many object fields (max 1048576)"
        ));
    }
    if (n->count == n->cap) {
        size_t cap = n->cap ? n->cap * 2 : 4;
        bloom_json_node **nd = (bloom_json_node **)realloc(n->items, cap * sizeof *nd);
        if (nd == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        n->items = nd;
        char **nk = (char **)realloc(n->keys, cap * sizeof *nk);
        if (nk == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        n->keys = nk;
        size_t *nl = (size_t *)realloc(n->key_lens, cap * sizeof *nl);
        if (nl == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        n->key_lens = nl;
        n->cap = cap;
    }
    char *key_copy = (char *)malloc(key.len + 1);
    if (key_copy == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(key_copy, key.data, key.len);
    key_copy[key.len] = '\0';
    n->keys[n->count] = key_copy;
    n->key_lens[n->count] = key.len;
    child->rc++;
    n->items[n->count] = child;
    n->count++;
    return bloom_json_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_json_push(
    bloom_struct_std_json_JsonValue v,
    bloom_struct_std_json_JsonValue item
) {
    bloom_json_node *n = bloom_json_unwrap(v);
    bloom_json_node *child = bloom_json_unwrap(item);
    if (n == NULL || child == NULL) {
        return bloom_json_err_bool(bloom_json_msg("push", "invalid JSON value"));
    }
    if (n->kind != BLOOM_JSON_ARRAY) {
        return bloom_json_err_bool(bloom_json_msg(
            "push",
            "value is not an array (create one with make_array)"
        ));
    }
    if (bloom_json_tree_contains(child, n)) {
        return bloom_json_err_bool(bloom_json_msg(
            "push",
            "this would create a cycle (the value already contains the array)"
        ));
    }
    if (n->count >= BLOOM_JSON_MAX_ELEMENTS) {
        return bloom_json_err_bool(bloom_json_msg(
            "push",
            "too many array elements (max 1048576)"
        ));
    }
    child->rc++;
    bloom_json_node_append(n, child);
    return bloom_json_ok_bool(true);
}

bloom_struct_std_json_JsonValue bloom_json_null(void) {
    return bloom_json_wrap(bloom_json_node_new(BLOOM_JSON_NULL));
}

bloom_struct_std_json_JsonValue bloom_json_make_bool(bool b) {
    bloom_json_node *n = bloom_json_node_new(BLOOM_JSON_BOOL);
    n->bval = b;
    return bloom_json_wrap(n);
}

bloom_struct_std_json_JsonValue bloom_json_make_int(int64_t v) {
    bloom_json_node *n = bloom_json_node_new(BLOOM_JSON_NUMBER);
    n->num_is_int = true;
    n->ival_ok = true;
    n->ival = v;
    return bloom_json_wrap(n);
}

bloom_enum_Result_std_json_JsonValue_str bloom_json_make_float(double v) {
    if (!isfinite(v)) {
        return bloom_json_err_value(bloom_json_msg(
            "make_float",
            "cannot represent a non-finite number (NaN or infinity)"
        ));
    }
    bloom_json_node *n = bloom_json_node_new(BLOOM_JSON_NUMBER);
    n->num_is_int = false;
    n->dval = v;
    return bloom_json_ok_value(bloom_json_wrap(n));
}

bloom_struct_std_json_JsonValue bloom_json_make_string(bloom_str s) {
    bloom_json_node *n = bloom_json_node_new(BLOOM_JSON_STRING);
    n->str = (char *)malloc(s.len + 1);
    if (n->str == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(n->str, s.data, s.len);
    n->str[s.len] = '\0';
    n->str_len = s.len;
    return bloom_json_wrap(n);
}

bloom_struct_std_json_JsonValue bloom_json_make_array(void) {
    return bloom_json_wrap(bloom_json_node_new(BLOOM_JSON_ARRAY));
}

bloom_struct_std_json_JsonValue bloom_json_make_object(void) {
    return bloom_json_wrap(bloom_json_node_new(BLOOM_JSON_OBJECT));
}

bloom_str bloom_json_type_of(bloom_struct_std_json_JsonValue v) {
    bloom_json_node *n = bloom_json_unwrap(v);
    if (n == NULL) {
        return bloom_str_from_bytes("invalid", 7);
    }
    const char *name = bloom_json_kind_name(n->kind);
    return bloom_str_from_bytes(name, strlen(name));
}
