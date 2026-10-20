/* Bloom std::html runtime — HTML escaping and a small template engine.
 *
 * Security model (documented limitations):
 *   - Values inserted with `{{path}}` are always HTML-escaped (`&`, `<`,
 *     `>`, `"`, `'`). Escaping protects HTML text and quoted attribute
 *     values; it does NOT make JavaScript, CSS or URL contexts safe, and it
 *     does not protect unquoted attribute values. Templates that contain
 *     `<script>` or `<style>` blocks are rejected at parse time; there is
 *     no raw-output placeholder.
 *
 * Template syntax:
 *   {{path}}              insert an escaped value (path: `a.b.c`; inside
 *                         `#each` the prefix `this` addresses the current
 *                         element, e.g. `{{this.name}}` or `{{this}}`)
 *   {{#if path}}...{{/if}}   boolean section (strict: value must be bool)
 *   {{#if path}}...{{else}}...{{/if}}
 *   {{#each path}}...{{/each}}  iterate an array (strict: value must be
 *                         an array); `{{this}}` is the current element
 *   Directives are exactly `if`, `each` and `else`; blocks nest up to 32
 *   levels. `this` is only available inside `#each` and refers to the
 *   innermost loop element (outer elements are not addressable).
 *
 * Value rules: strings, numbers (parsed literals keep their text) and
 * booleans insert text; `null`, arrays and objects are render errors (no
 * implicit debug output). Missing paths are render errors; unknown
 * directives, unclosed blocks and malformed placeholders are parse errors
 * with byte/line/column positions.
 *
 * Limits (enforced, never silently truncated):
 *   source 65536 bytes, template parts 16384, nesting 32, path 256 bytes
 *   with at most 32 segments, rendered output 1048576 bytes, total loop
 *   iterations 65536.
 */

#define BLOOM_HTML_MAX_SOURCE (64 * 1024)
#define BLOOM_HTML_MAX_NODES 16384
#define BLOOM_HTML_MAX_DEPTH 32
#define BLOOM_HTML_MAX_OUTPUT (1024 * 1024)
#define BLOOM_HTML_MAX_ITERATIONS 65536
#define BLOOM_HTML_MAX_PATH 256
#define BLOOM_HTML_MAX_PATH_SEGMENTS 32
#define BLOOM_HTML_MAX_SEGMENT 64

enum {
    BLOOM_HTML_TEXT = 0,
    BLOOM_HTML_VAR = 1,
    BLOOM_HTML_IF = 2,
    BLOOM_HTML_ELSE = 3,
    BLOOM_HTML_EACH = 4,
    BLOOM_HTML_END = 5
};

typedef struct {
    int kind;
    char *text; /* TEXT: literal bytes; VAR/IF/EACH: path */
    size_t len;
    size_t jump; /* IF: ELSE/END; ELSE: END; EACH: END */
    bool is_each; /* END: closes an each block */
} bloom_html_node;

typedef struct {
    size_t rc;
    bloom_html_node *nodes;
    size_t count;
} bloom_html_tmpl;

/* ---- small helpers ------------------------------------------------------ */

static bloom_str bloom_html_msg(const char *op, const char *detail) {
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

/* Builds "prefix + path + suffix" without truncation (the buffer is sized
 * from the actual parts, so no message can be cut off). */
static bloom_str bloom_html_path_msg(
    const char *prefix,
    const char *path,
    size_t path_len,
    const char *suffix
) {
    size_t total = strlen(prefix) + path_len + strlen(suffix) + 1;
    char *buf = (char *)malloc(total);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    snprintf(buf, total, "%s%.*s%s", prefix, (int)path_len, path, suffix);
    bloom_str s = bloom_str_from_bytes(buf, strlen(buf));
    free(buf);
    return s;
}

static bloom_enum_Result_std_html_Template_str bloom_html_ok_template(
    bloom_struct_std_html_Template v
) {
    bloom_enum_Result_std_html_Template_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_std_html_Template_str bloom_html_err_template(bloom_str m) {
    bloom_enum_Result_std_html_Template_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_str_str bloom_html_ok_str(bloom_str v) {
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_str_str bloom_html_err_str(bloom_str m) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_bool_str bloom_html_ok_bool(bool v) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_bool_str bloom_html_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_struct_std_html_Template bloom_html_wrap(bloom_html_tmpl *t) {
    bloom_struct_std_html_Template v;
    v.f_id = (bloom_html_handle)(intptr_t)t;
    return v;
}

static bloom_html_tmpl *bloom_html_unwrap(bloom_struct_std_html_Template v) {
    return (bloom_html_tmpl *)(intptr_t)v.f_id;
}

bloom_html_handle bloom_html_retain(bloom_html_handle h) {
    bloom_html_tmpl *t = (bloom_html_tmpl *)(intptr_t)h;
    if (t != NULL) {
        t->rc++;
    }
    return h;
}

void bloom_html_release(bloom_html_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_html_tmpl *t = (bloom_html_tmpl *)(intptr_t)*h;
    *h = 0;
    if (t->rc > 0) {
        t->rc--;
    }
    if (t->rc == 0) {
        for (size_t i = 0; i < t->count; i++) {
            free(t->nodes[i].text);
        }
        free(t->nodes);
        free(t);
        bloom_live_allocs--;
    }
}

/* ---- growable output buffer with a hard cap ----------------------------- */

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} bloom_html_buf;

static void bloom_html_buf_init(bloom_html_buf *b) {
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
}

static void bloom_html_buf_free(bloom_html_buf *b) {
    free(b->data);
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
}

/* Appends raw bytes; returns false when the output limit would be exceeded
 * (nothing is appended in that case). */
static bool bloom_html_buf_append(bloom_html_buf *b, const char *p, size_t n) {
    if (n == 0) {
        return true;
    }
    if (n > BLOOM_HTML_MAX_OUTPUT - b->len) {
        return false;
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
    return true;
}

/* Appends `p` with HTML escaping (&, <, >, ", '). */
static bool bloom_html_buf_append_escaped(bloom_html_buf *b, const char *p, size_t n) {
    size_t i = 0;
    while (i < n) {
        char c = p[i];
        const char *entity = NULL;
        switch (c) {
            case '&': entity = "&amp;"; break;
            case '<': entity = "&lt;"; break;
            case '>': entity = "&gt;"; break;
            case '"': entity = "&quot;"; break;
            case '\'': entity = "&#39;"; break;
            default: break;
        }
        if (entity != NULL) {
            if (!bloom_html_buf_append(b, entity, strlen(entity))) {
                return false;
            }
        } else if (!bloom_html_buf_append(b, p + i, 1)) {
            return false;
        }
        i++;
    }
    return true;
}

/* ---- HTML escaping (public) --------------------------------------------- */

bloom_str bloom_html_escape(bloom_str text) {
    /* Worst case: every byte becomes `&quot;` (6 bytes). */
    if (text.len > (SIZE_MAX - 1) / 6) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t cap = text.len * 6 + 1;
    char *buf = (char *)malloc(cap);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t at = 0;
    for (size_t i = 0; i < text.len; i++) {
        char c = text.data[i];
        const char *entity = NULL;
        switch (c) {
            case '&': entity = "&amp;"; break;
            case '<': entity = "&lt;"; break;
            case '>': entity = "&gt;"; break;
            case '"': entity = "&quot;"; break;
            case '\'': entity = "&#39;"; break;
            default: break;
        }
        if (entity != NULL) {
            size_t en = strlen(entity);
            memcpy(buf + at, entity, en);
            at += en;
        } else {
            buf[at++] = c;
        }
    }
    bloom_str out = bloom_str_from_bytes(buf, at);
    free(buf);
    return out;
}

/* ---- parser -------------------------------------------------------------- */

typedef struct {
    size_t node;
    bool is_each;
    bool seen_else;
    size_t else_index;
    size_t pos; /* byte offset of the opening tag (for error messages) */
} bloom_html_open;

static bloom_str bloom_html_parse_error(
    const char *source,
    size_t len,
    size_t pos,
    const char *reason
) {
    size_t line = 1;
    size_t col = 1;
    size_t stop = pos < len ? pos : len;
    for (size_t i = 0; i < stop; i++) {
        if (source[i] == '\n') {
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
    size_t rn = strlen(reason);
    size_t pn = strlen(posbuf);
    char *buf = (char *)malloc(rn + pn + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(buf, reason, rn);
    memcpy(buf + rn, posbuf, pn);
    bloom_str msg = bloom_str_from_bytes(buf, rn + pn);
    free(buf);
    return msg;
}

static bool bloom_html_valid_path(const char *p, size_t n) {
    if (n == 0 || n > BLOOM_HTML_MAX_PATH) {
        return false;
    }
    size_t segments = 1;
    size_t seg_len = 0;
    for (size_t i = 0; i < n; i++) {
        char c = p[i];
        if (c == '.') {
            if (seg_len == 0 || seg_len > BLOOM_HTML_MAX_SEGMENT) {
                return false;
            }
            segments++;
            if (segments > BLOOM_HTML_MAX_PATH_SEGMENTS) {
                return false;
            }
            seg_len = 0;
            continue;
        }
        bool first = seg_len == 0;
        if (first) {
            if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_')) {
                return false;
            }
        } else if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
                     || (c >= '0' && c <= '9') || c == '_')) {
            return false;
        }
        seg_len++;
    }
    return seg_len > 0 && seg_len <= BLOOM_HTML_MAX_SEGMENT;
}

/* Case-insensitive search for `<script` / `<style`. */
static bool bloom_html_contains_unsafe_block(
    const char *s,
    size_t n,
    size_t *found_at
) {
    for (size_t i = 0; i + 7 <= n; i++) {
        if (s[i] != '<') {
            continue;
        }
        const char *rest = s + i + 1;
        size_t left = n - i - 1;
        const char *names[2] = {"script", "style"};
        for (int k = 0; k < 2; k++) {
            size_t len = strlen(names[k]);
            if (left < len) {
                continue;
            }
            bool match = true;
            for (size_t j = 0; j < len && match; j++) {
                char c = rest[j];
                if (c >= 'A' && c <= 'Z') {
                    c = (char)(c + 32);
                }
                if (c != names[k][j]) {
                    match = false;
                }
            }
            if (match) {
                *found_at = i;
                return true;
            }
        }
    }
    return false;
}

static bool bloom_html_node_add(
    bloom_html_node **nodes,
    size_t *count,
    size_t *cap,
    int kind,
    const char *text,
    size_t len
) {
    if (*count >= BLOOM_HTML_MAX_NODES) {
        return false;
    }
    if (*count == *cap) {
        size_t ncap = *cap ? *cap * 2 : 64;
        bloom_html_node *nd = (bloom_html_node *)realloc(
            *nodes,
            ncap * sizeof *nd
        );
        if (nd == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        *nodes = nd;
        *cap = ncap;
    }
    bloom_html_node *n = &(*nodes)[*count];
    memset(n, 0, sizeof *n);
    n->kind = kind;
    n->len = len;
    if (text != NULL && len > 0) {
        n->text = (char *)malloc(len + 1);
        if (n->text == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        memcpy(n->text, text, len);
        n->text[len] = '\0';
    }
    (*count)++;
    return true;
}

static bool bloom_html_is_space(char c) {
    return c == ' ' || c == '\t';
}

static bool bloom_html_starts_with(const char *p, size_t n, const char *prefix) {
    size_t pn = strlen(prefix);
    return n >= pn && memcmp(p, prefix, pn) == 0;
}

bloom_enum_Result_std_html_Template_str bloom_html_template(bloom_str source) {
    if (source.len > BLOOM_HTML_MAX_SOURCE) {
        return bloom_html_err_template(bloom_html_msg(
            "template",
            "template source exceeds 65536 bytes"
        ));
    }
    const char *src = source.data != NULL ? source.data : "";
    size_t len = source.len;
    size_t unsafe_at = 0;
    if (bloom_html_contains_unsafe_block(src, len, &unsafe_at)) {
        return bloom_html_err_template(bloom_html_parse_error(
            src,
            len,
            unsafe_at,
            "`<script>` and `<style>` blocks are not supported in templates"
        ));
    }
    bloom_html_node *nodes = NULL;
    size_t count = 0;
    size_t cap = 0;
    bloom_html_open open[BLOOM_HTML_MAX_DEPTH];
    size_t depth = 0;
    size_t pos = 0;
    for (;;) {
        size_t found = len;
        for (size_t i = pos; i + 1 < len; i++) {
            if (src[i] == '{' && src[i + 1] == '{') {
                found = i;
                break;
            }
        }
        if (found >= len) {
            if (pos < len) {
                if (!bloom_html_node_add(&nodes, &count, &cap, BLOOM_HTML_TEXT, src + pos, len - pos)) {
                    bloom_str msg = bloom_html_parse_error(src, len, pos, "template has too many parts (max 16384)");
                    bloom_enum_Result_std_html_Template_str r;
                    r.tag = 1;
                    r.data.m_Err._0 = msg;
                    for (size_t i = 0; i < count; i++) {
                        free(nodes[i].text);
                    }
                    free(nodes);
                    return r;
                }
            }
            break;
        }
        if (found > pos) {
            if (!bloom_html_node_add(&nodes, &count, &cap, BLOOM_HTML_TEXT, src + pos, found - pos)) {
                bloom_str msg = bloom_html_parse_error(src, len, found, "template has too many parts (max 16384)");
                bloom_enum_Result_std_html_Template_str r;
                r.tag = 1;
                r.data.m_Err._0 = msg;
                for (size_t i = 0; i < count; i++) {
                    free(nodes[i].text);
                }
                free(nodes);
                return r;
            }
        }
        size_t content_start = found + 2;
        size_t close = len;
        for (size_t i = content_start; i + 1 < len; i++) {
            if (src[i] == '}' && src[i + 1] == '}') {
                close = i;
                break;
            }
        }
        if (close >= len) {
            bloom_str msg = bloom_html_parse_error(
                src,
                len,
                found,
                "unterminated placeholder (missing `}}`)"
            );
            for (size_t i = 0; i < count; i++) {
                free(nodes[i].text);
            }
            free(nodes);
            bloom_enum_Result_std_html_Template_str r;
            r.tag = 1;
            r.data.m_Err._0 = msg;
            return r;
        }
        size_t cs = content_start;
        size_t ce = close;
        while (cs < ce && bloom_html_is_space(src[cs])) {
            cs++;
        }
        while (ce > cs && bloom_html_is_space(src[ce - 1])) {
            ce--;
        }
        const char *inner = src + cs;
        size_t inner_len = ce - cs;
        enum { HTML_ADD_OK, HTML_ADD_ERR, HTML_ADD_OUT_OF_ROOM } added = HTML_ADD_OK;
        bloom_str err_msg;
        err_msg.rc = 0;
        err_msg.data = NULL;
        err_msg.len = 0;
        if (inner_len == 0) {
            err_msg = bloom_html_parse_error(src, len, found, "empty placeholder");
            added = HTML_ADD_ERR;
        } else if (inner[0] == '#') {
            /* Directive: `#if path` / `#each path` */
            size_t at = 1;
            while (at < inner_len && bloom_html_is_space(inner[at])) {
                at++;
            }
            size_t word_start = at;
            while (at < inner_len && inner[at] >= 'a' && inner[at] <= 'z') {
                at++;
            }
            size_t word_len = at - word_start;
            const char *word = inner + word_start;
            bool is_if = word_len == 2 && memcmp(word, "if", 2) == 0;
            bool is_each = word_len == 4 && memcmp(word, "each", 4) == 0;
            if (!is_if && !is_each) {
                err_msg = bloom_html_parse_error(
                    src,
                    len,
                    found,
                    "unknown directive (only `#if` and `#each` are supported)"
                );
                added = HTML_ADD_ERR;
            } else if (at >= inner_len || !bloom_html_is_space(inner[at])) {
                err_msg = bloom_html_parse_error(
                    src,
                    len,
                    found,
                    "directive requires a path (for example `{{#if enabled}}`)"
                );
                added = HTML_ADD_ERR;
            } else {
                while (at < inner_len && bloom_html_is_space(inner[at])) {
                    at++;
                }
                size_t path_start = at;
                while (at < inner_len) {
                    at++;
                }
                while (at > path_start && bloom_html_is_space(inner[at - 1])) {
                    at--;
                }
                if (!bloom_html_valid_path(inner + path_start, at - path_start)) {
                    err_msg = bloom_html_parse_error(
                        src,
                        len,
                        found,
                        "invalid path (expected `name` or `a.b.c`)"
                    );
                    added = HTML_ADD_ERR;
                } else if (depth >= BLOOM_HTML_MAX_DEPTH) {
                    err_msg = bloom_html_parse_error(
                        src,
                        len,
                        found,
                        "template nests too deeply (max 32 levels)"
                    );
                    added = HTML_ADD_ERR;
                } else {
                    int kind = is_each ? BLOOM_HTML_EACH : BLOOM_HTML_IF;
                    if (!bloom_html_node_add(
                            &nodes,
                            &count,
                            &cap,
                            kind,
                            inner + path_start,
                            at - path_start)) {
                        err_msg = bloom_html_parse_error(src, len, found, "template has too many parts (max 16384)");
                        added = HTML_ADD_ERR;
                    } else {
                        open[depth].node = count - 1;
                        open[depth].is_each = is_each;
                        open[depth].seen_else = false;
                        open[depth].else_index = 0;
                        open[depth].pos = found;
                        depth++;
                    }
                }
            }
        } else if (inner[0] == '/') {
            size_t at = 1;
            while (at < inner_len && bloom_html_is_space(inner[at])) {
                at++;
            }
            size_t word_start = at;
            while (at < inner_len && inner[at] >= 'a' && inner[at] <= 'z') {
                at++;
            }
            size_t word_len = at - word_start;
            const char *word = inner + word_start;
            while (at < inner_len && bloom_html_is_space(inner[at])) {
                at++;
            }
            bool is_if = word_len == 2 && memcmp(word, "if", 2) == 0;
            bool is_each = word_len == 4 && memcmp(word, "each", 4) == 0;
            if ((!is_if && !is_each) || at != inner_len) {
                err_msg = bloom_html_parse_error(
                    src,
                    len,
                    found,
                    "unknown closing tag (only `{{/if}}` and `{{/each}}` are supported)"
                );
                added = HTML_ADD_ERR;
            } else if (depth == 0 || open[depth - 1].is_each != is_each) {
                err_msg = bloom_html_parse_error(
                    src,
                    len,
                    found,
                    "closing tag does not match the open block"
                );
                added = HTML_ADD_ERR;
            } else {
                if (!bloom_html_node_add(&nodes, &count, &cap, BLOOM_HTML_END, NULL, 0)) {
                    err_msg = bloom_html_parse_error(src, len, found, "template has too many parts (max 16384)");
                    added = HTML_ADD_ERR;
                } else {
                    bloom_html_node *end = &nodes[count - 1];
                    end->is_each = is_each;
                    bloom_html_node *opener = &nodes[open[depth - 1].node];
                    if (!is_each && open[depth - 1].seen_else) {
                        bloom_html_node *else_n = &nodes[open[depth - 1].else_index];
                        else_n->jump = count - 1;
                    } else {
                        opener->jump = count - 1;
                    }
                    end->jump = open[depth - 1].node;
                    depth--;
                }
            }
        } else if (inner_len == 4 && memcmp(inner, "else", 4) == 0) {
            if (depth == 0 || open[depth - 1].is_each || open[depth - 1].seen_else) {
                err_msg = bloom_html_parse_error(
                    src,
                    len,
                    found,
                    "`{{else}}` is only allowed once inside `{{#if}}`"
                );
                added = HTML_ADD_ERR;
            } else if (!bloom_html_node_add(&nodes, &count, &cap, BLOOM_HTML_ELSE, NULL, 0)) {
                err_msg = bloom_html_parse_error(src, len, found, "template has too many parts (max 16384)");
                added = HTML_ADD_ERR;
            } else {
                bloom_html_node *opener = &nodes[open[depth - 1].node];
                /* The false branch continues after the `{{else}}` marker. */
                open[depth - 1].else_index = count - 1;
                opener->jump = count;
                open[depth - 1].seen_else = true;
            }
        } else {
            /* Plain value placeholder. */
            if (!bloom_html_valid_path(inner, inner_len)) {
                err_msg = bloom_html_parse_error(
                    src,
                    len,
                    found,
                    "invalid placeholder (expected `name` or `a.b.c`)"
                );
                added = HTML_ADD_ERR;
            } else if (!bloom_html_node_add(
                           &nodes,
                           &count,
                           &cap,
                           BLOOM_HTML_VAR,
                           inner,
                           inner_len)) {
                err_msg = bloom_html_parse_error(src, len, found, "template has too many parts (max 16384)");
                added = HTML_ADD_ERR;
            }
        }
        (void)added;
        if (err_msg.data != NULL) {
            for (size_t i = 0; i < count; i++) {
                free(nodes[i].text);
            }
            free(nodes);
            bloom_enum_Result_std_html_Template_str r;
            r.tag = 1;
            r.data.m_Err._0 = err_msg;
            return r;
        }
        pos = close + 2;
        if (pos >= len) {
            break;
        }
    }
    if (depth != 0) {
        bloom_str msg = bloom_html_parse_error(
            src,
            len,
            open[depth - 1].pos,
            "unclosed block (missing `{{/if}}` or `{{/each}}`)"
        );
        for (size_t i = 0; i < count; i++) {
            free(nodes[i].text);
        }
        free(nodes);
        bloom_enum_Result_std_html_Template_str r;
        r.tag = 1;
        r.data.m_Err._0 = msg;
        return r;
    }
    bloom_html_tmpl *t = (bloom_html_tmpl *)calloc(1, sizeof *t);
    if (t == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    t->rc = 1;
    t->nodes = nodes;
    t->count = count;
    bloom_live_allocs++;
    return bloom_html_ok_template(bloom_html_wrap(t));
}

/* ---- renderer ------------------------------------------------------------ */

typedef struct {
    int kind; /* BLOOM_HTML_IF or BLOOM_HTML_EACH */
    size_t start;
    bloom_json_node *array;
    size_t index;
} bloom_html_loop;

typedef struct {
    bloom_json_node *root;
    bloom_html_loop stack[BLOOM_HTML_MAX_DEPTH];
    size_t depth;
    size_t iterations;
    bloom_html_buf out;
} bloom_html_render_state;

static bool bloom_html_path_equal(const char *p, size_t n, const char *name) {
    size_t name_len = strlen(name);
    return n == name_len && memcmp(p, name, name_len) == 0;
}

/* Resolves `path` against the root object and the innermost loop element.
 * Returns NULL and sets `err` when a segment is missing or the value in the
 * way is not an object. */
static bloom_json_node *bloom_html_resolve(
    bloom_html_render_state *st,
    const char *path,
    size_t path_len,
    bloom_str *err
) {
    size_t at = 0;
    size_t seg_end = 0;
    while (seg_end < path_len && path[seg_end] != '.') {
        seg_end++;
    }
    size_t seg_len = seg_end - at;
    bloom_json_node *cur = NULL;
    if (!bloom_html_path_equal(path, seg_len, "this")) {
        cur = st->root;
    } else {
        /* Innermost `#each` element. */
        bloom_json_node *arr = NULL;
        size_t idx = 0;
        for (size_t i = st->depth; i > 0; i--) {
            if (st->stack[i - 1].kind == BLOOM_HTML_EACH) {
                arr = st->stack[i - 1].array;
                idx = st->stack[i - 1].index;
                break;
            }
        }
        if (arr == NULL) {
            *err = bloom_html_msg("render", "`this` is only available inside `#each`");
            return NULL;
        }
        cur = arr->items[idx];
        at = seg_end + 1;
        if (at >= path_len) {
            return cur;
        }
    }
    while (at < path_len) {
        size_t next = at;
        while (next < path_len && path[next] != '.') {
            next++;
        }
        size_t len = next - at;
        if (cur->kind != BLOOM_JSON_OBJECT) {
            *err = bloom_html_path_msg(
                "render: `",
                path,
                path_len,
                "` is not an object"
            );
            return NULL;
        }
        bloom_json_node *child = bloom_json_object_find(cur, path + at, len);
        if (child == NULL) {
            *err = bloom_html_path_msg(
                "render: missing value for `",
                path,
                path_len,
                "`"
            );
            return NULL;
        }
        cur = child;
        at = next + 1;
    }
    return cur;
}

static bool bloom_html_append_value(
    bloom_html_render_state *st,
    bloom_json_node *n,
    const char *path,
    size_t path_len,
    bloom_str *err
) {
    switch (n->kind) {
        case BLOOM_JSON_STRING:
            if (!bloom_html_buf_append_escaped(&st->out, n->str, n->str_len)) {
                *err = bloom_html_msg("render", "rendered output exceeds 1048576 bytes");
                return false;
            }
            return true;
        case BLOOM_JSON_NUMBER: {
            char tmp[64];
            if (n->num != NULL) {
                if (!bloom_html_buf_append(&st->out, n->num, n->num_len)) {
                    *err = bloom_html_msg("render", "rendered output exceeds 1048576 bytes");
                    return false;
                }
                return true;
            }
            if (n->num_is_int) {
                snprintf(tmp, sizeof tmp, "%" PRId64, n->ival);
            } else {
                if (!isfinite(n->dval)) {
                    *err = bloom_html_msg("render", "cannot render a non-finite number");
                    return false;
                }
                bloom_json_format_double(n->dval, tmp, sizeof tmp);
            }
            if (!bloom_html_buf_append(&st->out, tmp, strlen(tmp))) {
                *err = bloom_html_msg("render", "rendered output exceeds 1048576 bytes");
                return false;
            }
            return true;
        }
        case BLOOM_JSON_BOOL:
            if (!bloom_html_buf_append(&st->out, n->bval ? "true" : "false", n->bval ? 4 : 5)) {
                *err = bloom_html_msg("render", "rendered output exceeds 1048576 bytes");
                return false;
            }
            return true;
        case BLOOM_JSON_NULL: {
            *err = bloom_html_path_msg(
                "render: value for `",
                path,
                path_len,
                "` is null"
            );
            return false;
        }
        default: {
            *err = bloom_html_path_msg(
                "render: `",
                path,
                path_len,
                n->kind == BLOOM_JSON_ARRAY
                    ? "` is an array; only strings, numbers and booleans can be inserted"
                    : "` is an object; only strings, numbers and booleans can be inserted"
            );
            return false;
        }
    }
}

bloom_enum_Result_str_str bloom_html_render(
    bloom_struct_std_html_Template tmpl,
    bloom_struct_std_json_JsonValue data
) {
    bloom_html_tmpl *t = bloom_html_unwrap(tmpl);
    bloom_json_node *root = bloom_json_unwrap(data);
    if (t == NULL) {
        return bloom_html_err_str(bloom_html_msg("render", "invalid template"));
    }
    if (root == NULL) {
        return bloom_html_err_str(bloom_html_msg("render", "invalid data value"));
    }
    if (root->kind != BLOOM_JSON_OBJECT) {
        return bloom_html_err_str(bloom_html_msg(
            "render",
            "data must be a JSON object"
        ));
    }
    bloom_html_render_state st;
    memset(&st, 0, sizeof st);
    st.root = root;
    bloom_html_buf_init(&st.out);
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    size_t pc = 0;
    bool ok = true;
    while (pc < t->count) {
        bloom_html_node *n = &t->nodes[pc];
        if (n->kind == BLOOM_HTML_TEXT) {
            if (!bloom_html_buf_append(&st.out, n->text, n->len)) {
                err = bloom_html_msg("render", "rendered output exceeds 1048576 bytes");
                ok = false;
                break;
            }
            pc++;
            continue;
        }
        if (n->kind == BLOOM_HTML_VAR) {
            bloom_json_node *value = bloom_html_resolve(&st, n->text, n->len, &err);
            if (value == NULL) {
                ok = false;
                break;
            }
            if (!bloom_html_append_value(&st, value, n->text, n->len, &err)) {
                ok = false;
                break;
            }
            pc++;
            continue;
        }
        if (n->kind == BLOOM_HTML_IF) {
            bloom_json_node *value = bloom_html_resolve(&st, n->text, n->len, &err);
            if (value == NULL) {
                ok = false;
                break;
            }
            if (value->kind != BLOOM_JSON_BOOL) {
                err = bloom_html_path_msg(
                    "render: `",
                    n->text,
                    n->len,
                    "` is not a boolean (the condition must be true or false)"
                );
                ok = false;
                break;
            }
            st.stack[st.depth].kind = BLOOM_HTML_IF;
            st.stack[st.depth].start = pc;
            st.stack[st.depth].array = NULL;
            st.stack[st.depth].index = 0;
            st.depth++;
            if (value->bval) {
                pc++;
            } else {
                pc = n->jump; /* ELSE or END */
            }
            continue;
        }
        if (n->kind == BLOOM_HTML_ELSE) {
            pc = n->jump; /* END */
            continue;
        }
        if (n->kind == BLOOM_HTML_EACH) {
            bloom_json_node *value = bloom_html_resolve(&st, n->text, n->len, &err);
            if (value == NULL) {
                ok = false;
                break;
            }
            if (value->kind != BLOOM_JSON_ARRAY) {
                err = bloom_html_path_msg(
                    "render: `",
                    n->text,
                    n->len,
                    "` is not an array (only arrays can be iterated)"
                );
                ok = false;
                break;
            }
            st.stack[st.depth].kind = BLOOM_HTML_EACH;
            st.stack[st.depth].start = pc;
            st.stack[st.depth].array = value;
            st.stack[st.depth].index = 0;
            st.depth++;
            if (value->count == 0) {
                pc = n->jump; /* END pops and continues */
            } else {
                st.iterations++;
                if (st.iterations > BLOOM_HTML_MAX_ITERATIONS) {
                    err = bloom_html_msg("render", "too many loop iterations (max 65536)");
                    ok = false;
                    break;
                }
                pc++;
            }
            continue;
        }
        /* END */
        {
            st.depth--;
            bloom_html_loop state = st.stack[st.depth];
            if (state.kind == BLOOM_HTML_EACH) {
                state.index++;
                if (state.index < state.array->count) {
                    st.iterations++;
                    if (st.iterations > BLOOM_HTML_MAX_ITERATIONS) {
                        err = bloom_html_msg("render", "too many loop iterations (max 65536)");
                        ok = false;
                        break;
                    }
                    st.stack[st.depth] = state;
                    st.depth++;
                    pc = state.start + 1;
                    continue;
                }
            }
            pc++;
            continue;
        }
    }
    if (!ok) {
        bloom_html_buf_free(&st.out);
        return bloom_html_err_str(err);
    }
    bloom_str result = bloom_str_from_bytes(
        st.out.data != NULL ? st.out.data : "",
        st.out.len
    );
    bloom_html_buf_free(&st.out);
    return bloom_html_ok_str(result);
}

/* ---- HTTP integration ---------------------------------------------------- */

bloom_enum_Result_bool_str bloom_html_render_page_with(
    int64_t connection,
    int64_t status,
    bloom_struct_std_html_Template tmpl,
    bloom_struct_std_json_JsonValue data,
    bloom_str headers
);

bloom_enum_Result_bool_str bloom_html_render_page(
    int64_t connection,
    int64_t status,
    bloom_struct_std_html_Template tmpl,
    bloom_struct_std_json_JsonValue data
) {
    bloom_str empty;
    empty.rc = 0;
    empty.data = NULL;
    empty.len = 0;
    return bloom_html_render_page_with(connection, status, tmpl, data, empty);
}

bloom_enum_Result_bool_str bloom_html_render_page_with(
    int64_t connection,
    int64_t status,
    bloom_struct_std_html_Template tmpl,
    bloom_struct_std_json_JsonValue data,
    bloom_str headers
) {
    bloom_enum_Result_str_str text = bloom_html_render(tmpl, data);
    if (text.tag == 1) {
        return bloom_html_err_bool(text.data.m_Err._0);
    }
    bloom_str html_type = bloom_str_from_bytes("text/html; charset=utf-8", 24);
    bloom_enum_Result_bool_str written = bloom_http_write_response_full(
        connection,
        status,
        html_type,
        headers,
        text.data.m_Ok._0
    );
    bloom_str_drop(&html_type);
    bloom_str_drop(&text.data.m_Ok._0);
    return written;
}

/* ---- loading templates from files ---------------------------------------- */

/* Loads a template from `path` (absolute or relative to the working
 * directory). The file must be valid UTF-8 and at most 65536 bytes; the
 * same parser and limits as `template(source)` apply. Files are loaded
 * once; there is no file watching or reloading. */
bloom_enum_Result_std_html_Template_str bloom_html_template_file(bloom_str path) {
    bloom_enum_Result_str_str text = bloom_fs_read_text_limited(
        path,
        BLOOM_HTML_MAX_SOURCE
    );
    if (text.tag == 1) {
        bloom_str detail = text.data.m_Err._0;
        size_t total = 15 + detail.len + 1;
        char *buf = (char *)malloc(total);
        if (buf == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        memcpy(buf, "template_file: ", 15);
        memcpy(buf + 15, detail.data, detail.len);
        bloom_str msg = bloom_str_from_bytes(buf, 15 + detail.len);
        free(buf);
        bloom_str_drop(&detail);
        return bloom_html_err_template(msg);
    }
    bloom_enum_Result_std_html_Template_str parsed = bloom_html_template(text.data.m_Ok._0);
    bloom_str_drop(&text.data.m_Ok._0);
    return parsed;
}

/* Loads and parses a template from `root` + a relative path. The relative
 * path is validated (no absolute paths, drive/UNC prefixes, `..` or empty
 * segments) and the opened file must resolve inside `root` (symbolic links
 * and junctions are resolved before the check). */
bloom_enum_Result_std_html_Template_str bloom_html_template_file_from(
    bloom_str root,
    bloom_str relative
) {
    char *joined = NULL;
    size_t joined_len = 0;
    bloom_str join_err;
    join_err.rc = 0;
    join_err.data = NULL;
    join_err.len = 0;
    if (!bloom_fs_join_inside_root(root, relative, &joined, &joined_len, &join_err)) {
        return bloom_html_err_template(join_err);
    }
    bloom_str path = bloom_str_from_bytes(joined, joined_len);
    free(joined);
    FILE *file = NULL;
    size_t size = 0;
    bloom_str open_err;
    open_err.rc = 0;
    open_err.data = NULL;
    open_err.len = 0;
    bloom_fs_open_status st = bloom_fs_open_confined(
        root,
        path,
        BLOOM_HTML_MAX_SOURCE,
        &file,
        &size,
        &open_err
    );
    if (st != BLOOM_FS_OPEN_OK) {
        bloom_str_drop(&path);
        size_t total = 17 + open_err.len + 1;
        char *buf = (char *)malloc(total);
        if (buf == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        memcpy(buf, "template_file_from: ", 20);
        memcpy(buf + 20, open_err.data, open_err.len);
        bloom_str msg = bloom_str_from_bytes(buf, 20 + open_err.len);
        free(buf);
        bloom_str_drop(&open_err);
        return bloom_html_err_template(msg);
    }
    unsigned char *data = NULL;
    size_t data_len = 0;
    bool ok = bloom_fs_read_all(file, size, &data, &data_len);
    fclose(file);
    bloom_str_drop(&path);
    if (!ok) {
        return bloom_html_err_template(bloom_html_msg(
            "template_file_from",
            "read failed"
        ));
    }
    if (!bloom_fs_utf8_valid(data, data_len)) {
        free(data);
        return bloom_html_err_template(bloom_html_msg(
            "template_file_from",
            "template file is not valid UTF-8"
        ));
    }
    bloom_str source = bloom_str_from_bytes((const char *)data, data_len);
    free(data);
    bloom_enum_Result_std_html_Template_str parsed = bloom_html_template(source);
    bloom_str_drop(&source);
    return parsed;
}
