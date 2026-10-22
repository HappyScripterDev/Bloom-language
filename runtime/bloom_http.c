/* Bloom std::http runtime — HTTP/1.0 and HTTP/1.1 with optional TLS.
 *
 * Only emitted when the program loads `std::http` (the net and tls runtimes
 * are emitted before it).
 *
 * Transport abstraction: one request/response engine works over either a
 * plain TCP connection (`http://`) or a validated TLS connection
 * (`https://`). TLS uses the std::tls runtime (Windows Schannel); HTTPS
 * certificates are always verified — there is no insecure mode.
 *
 * Documented limits (rejected with an error, never truncated silently):
 *   - request/status line: 8192 bytes
 *   - header block:       16384 bytes
 *   - body:             4194304 bytes (4 MiB)
 *   - interim (1xx) responses: 5
 *   - trailers: 32
 * Chunked transfer encoding is decoded for responses and requests;
 * `Transfer-Encoding` combined with `Content-Length`, duplicate lengths and
 * unsupported encodings are rejected (request smuggling defenses).
 */

#define BLOOM_HTTP_MAX_LINE 8192
#define BLOOM_HTTP_MAX_HEADERS 16384
#define BLOOM_HTTP_MAX_BODY 4194304
#define BLOOM_HTTP_CLIENT_TIMEOUT_MS 15000
#define BLOOM_HTTP_MAX_INTERIM 5
#define BLOOM_HTTP_MAX_TRAILERS 32

static bloom_enum_Result_bool_str bloom_http_ok_bool(bool v) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_bool_str bloom_http_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_str bloom_http_msg(const char *op, const char *detail) {
    return bloom_net_msg(op, detail);
}

/* ---- growable byte buffer ---------------------------------------------- */

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} bloom_http_buf;

static void bloom_http_buf_init(bloom_http_buf *b) {
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
}

static void bloom_http_buf_append(bloom_http_buf *b, const char *p, size_t n) {
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

static void bloom_http_buf_free(bloom_http_buf *b) {
    free(b->data);
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
}

/* ---- transport --------------------------------------------------------- */

typedef struct {
    int kind; /* 0 = TCP, 1 = TLS */
    int64_t handle;
} bloom_http_conn;

static bloom_enum_Result_Option_str_str bloom_http_conn_read(
    bloom_http_conn c,
    int64_t max_bytes
) {
    if (c.kind == 0) {
        return bloom_net_tcp_read(c.handle, max_bytes);
    }
    return bloom_tls_read(c.handle, max_bytes);
}

static bloom_enum_Result_i64_str bloom_http_conn_write(bloom_http_conn c, bloom_str data) {
    if (c.kind == 0) {
        return bloom_net_tcp_write_all(c.handle, data);
    }
    return bloom_tls_write_all(c.handle, data);
}

static void bloom_http_conn_close(bloom_http_conn c) {
    if (c.kind == 0) {
        bloom_net_tcp_close(c.handle);
    } else {
        bloom_tls_close(c.handle);
    }
}

static void bloom_http_conn_set_timeout(bloom_http_conn c, int64_t ms) {
    if (c.kind == 0) {
        bloom_net_tcp_set_timeout(c.handle, ms);
    } else {
        bloom_tls_set_timeout(c.handle, ms);
    }
}

/* ---- small helpers ----------------------------------------------------- */

static bool bloom_http_is_token_char(unsigned char c) {
    if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')) {
        return true;
    }
    switch (c) {
        case '!': case '#': case '$': case '%': case '&': case '\'':
        case '*': case '+': case '-': case '.': case '^': case '_':
        case '`': case '|': case '~':
            return true;
        default:
            return false;
    }
}

static bool bloom_http_valid_token(const char *s, size_t n) {
    if (n == 0) {
        return false;
    }
    for (size_t i = 0; i < n; i++) {
        if (!bloom_http_is_token_char((unsigned char)s[i])) {
            return false;
        }
    }
    return true;
}

static bool bloom_http_eq_ci(const char *a, size_t an, const char *b, size_t bn) {
    if (an != bn) {
        return false;
    }
    for (size_t i = 0; i < an; i++) {
        char ca = a[i];
        char cb = b[i];
        if (ca >= 'A' && ca <= 'Z') {
            ca = (char)(ca + 32);
        }
        if (cb >= 'A' && cb <= 'Z') {
            cb = (char)(cb + 32);
        }
        if (ca != cb) {
            return false;
        }
    }
    return true;
}

/* ---- header block parsing --------------------------------------------- */

typedef struct {
    const char *name;
    size_t name_len;
    const char *value;
    size_t value_len;
} bloom_http_header;

static int bloom_http_parse_headers(
    const char *block,
    size_t len,
    bloom_http_header *out,
    int max
) {
    size_t i = 0;
    int count = 0;
    while (i < len) {
        size_t line_end = i;
        while (line_end < len && block[line_end] != '\n') {
            line_end++;
        }
        size_t line_len = line_end - i;
        if (line_len > 0 && block[i + line_len - 1] == '\r') {
            line_len--;
        }
        if (line_len == 0) {
            if (line_end == len) {
                break;
            }
            return -1;
        }
        if (line_len > BLOOM_HTTP_MAX_LINE) {
            return -1;
        }
        const char *line = block + i;
        size_t colon = 0;
        while (colon < line_len && line[colon] != ':') {
            colon++;
        }
        if (colon == 0 || colon >= line_len) {
            return -1;
        }
        if (!bloom_http_valid_token(line, colon)) {
            return -1;
        }
        size_t vs = colon + 1;
        while (vs < line_len && (line[vs] == ' ' || line[vs] == '\t')) {
            vs++;
        }
        size_t ve = line_len;
        while (ve > vs && (line[ve - 1] == ' ' || line[ve - 1] == '\t')) {
            ve--;
        }
        if (count >= max) {
            return -1;
        }
        out[count].name = line;
        out[count].name_len = colon;
        out[count].value = line + vs;
        out[count].value_len = ve - vs;
        count++;
        i = line_end + 1;
    }
    return count;
}

static bool bloom_http_header_find(
    const bloom_http_header *headers,
    int count,
    const char *name,
    const char **value,
    size_t *value_len
) {
    size_t n = strlen(name);
    for (int i = 0; i < count; i++) {
        if (bloom_http_eq_ci(headers[i].name, headers[i].name_len, name, n)) {
            if (value != NULL) {
                *value = headers[i].value;
                *value_len = headers[i].value_len;
            }
            return true;
        }
    }
    return false;
}

static int bloom_http_header_count(const bloom_http_header *headers, int count, const char *name) {
    size_t n = strlen(name);
    int found = 0;
    for (int i = 0; i < count; i++) {
        if (bloom_http_eq_ci(headers[i].name, headers[i].name_len, name, n)) {
            found++;
        }
    }
    return found;
}

static bool bloom_http_parse_length(const char *s, size_t n, int64_t *out) {
    if (n == 0 || n > 18) {
        return false;
    }
    int64_t v = 0;
    for (size_t i = 0; i < n; i++) {
        if (s[i] < '0' || s[i] > '9') {
            return false;
        }
        v = v * 10 + (s[i] - '0');
    }
    *out = v;
    return true;
}

/* Classifies the Transfer-Encoding header values: returns 1 for exactly
 * `chunked`, 0 for no header, -1 for anything unsupported/ambiguous. */
static int bloom_http_transfer_encoding(
    const bloom_http_header *headers,
    int count,
    const char **value,
    size_t *value_len
) {
    int te_count = bloom_http_header_count(headers, count, "Transfer-Encoding");
    if (te_count == 0) {
        return 0;
    }
    if (te_count > 1) {
        /* Multiple Transfer-Encoding headers: only a single `chunked`
         * token is supported, anything else is ambiguous. */
        return -1;
    }
    bloom_http_header_find(headers, count, "Transfer-Encoding", value, value_len);
    /* Trim and compare case-insensitively with "chunked". */
    size_t vs = 0;
    while (vs < *value_len && ((*value)[vs] == ' ' || (*value)[vs] == '\t')) {
        vs++;
    }
    size_t ve = *value_len;
    while (ve > vs && ((*value)[ve - 1] == ' ' || (*value)[ve - 1] == '\t')) {
        ve--;
    }
    if (bloom_http_eq_ci(*value + vs, ve - vs, "chunked", 7)) {
        return 1;
    }
    return -1;
}

/* ---- buffered reader over a transport --------------------------------- */

typedef struct {
    bloom_http_conn conn;
    bloom_http_buf buf;
    size_t pos;
    bool eof;
    const char *what;
} bloom_http_reader;

static void bloom_http_reader_init(
    bloom_http_reader *r,
    bloom_http_conn conn,
    const char *what
) {
    r->conn = conn;
    bloom_http_buf_init(&r->buf);
    r->pos = 0;
    r->eof = false;
    r->what = what;
}

static void bloom_http_reader_free(bloom_http_reader *r) {
    bloom_http_buf_free(&r->buf);
}

/* Fetches more bytes; returns false when the connection ended or failed. */
static bool bloom_http_reader_fill(bloom_http_reader *r, bloom_str *err, bool *clean_eof) {
    if (r->eof) {
        *clean_eof = true;
        return false;
    }
    bloom_enum_Result_Option_str_str got = bloom_http_conn_read(r->conn, 4096);
    if (got.tag == 1) {
        *err = got.data.m_Err._0;
        return false;
    }
    if (got.data.m_Ok._0.tag == 1) {
        r->eof = true;
        *clean_eof = true;
        return false;
    }
    bloom_str chunk = got.data.m_Ok._0.data.m_Some._0;
    bloom_http_buf_append(&r->buf, chunk.data, chunk.len);
    bloom_str_drop(&chunk);
    *clean_eof = false;
    return true;
}

/* Reads one CRLF-terminated line (without the terminator). */
static bool bloom_http_reader_line(
    bloom_http_reader *r,
    const char **line,
    size_t *len,
    bloom_str *err
) {
    size_t scanned = r->pos;
    for (;;) {
        while (scanned < r->buf.len) {
            if (r->buf.data[scanned] == '\n') {
                size_t line_len = scanned - r->pos;
                if (line_len > 0 && r->buf.data[r->pos + line_len - 1] == '\r') {
                    line_len--;
                }
                *line = r->buf.data + r->pos;
                *len = line_len;
                r->pos = scanned + 1;
                if (*len > BLOOM_HTTP_MAX_LINE) {
                    *err = bloom_http_msg(r->what, "line exceeds 8192 bytes");
                    return false;
                }
                return true;
            }
            scanned++;
        }
        bool clean = false;
        if (!bloom_http_reader_fill(r, err, &clean)) {
            if (clean) {
                *err = bloom_http_msg(r->what, "connection closed before the message was complete");
            }
            return false;
        }
    }
}

/* Reads exactly `n` bytes, appending them to `out`. */
static bool bloom_http_reader_exact(
    bloom_http_reader *r,
    size_t n,
    bloom_http_buf *out,
    bloom_str *err
) {
    while (n > 0) {
        if (r->pos < r->buf.len) {
            size_t avail = r->buf.len - r->pos;
            size_t take = avail < n ? avail : n;
            bloom_http_buf_append(out, r->buf.data + r->pos, take);
            r->pos += take;
            n -= take;
            continue;
        }
        bool clean = false;
        if (!bloom_http_reader_fill(r, err, &clean)) {
            if (clean) {
                *err = bloom_http_msg(r->what, "connection closed before the body was complete");
            }
            return false;
        }
    }
    return true;
}

/* Reads until the connection ends, appending to `out` (bounded). */
static bool bloom_http_reader_to_eof(
    bloom_http_reader *r,
    bloom_http_buf *out,
    bloom_str *err
) {
    for (;;) {
        if (r->pos < r->buf.len) {
            bloom_http_buf_append(out, r->buf.data + r->pos, r->buf.len - r->pos);
            r->pos = r->buf.len;
            if (out->len > BLOOM_HTTP_MAX_BODY) {
                *err = bloom_http_msg(r->what, "body exceeds 4194304 bytes");
                return false;
            }
        }
        bool clean = false;
        if (!bloom_http_reader_fill(r, err, &clean)) {
            return clean;
        }
    }
}

/* Decodes a chunked body (RFC 7230 §4.1). Trailer fields are validated and
 * discarded (they are not exposed in HttpRequest/HttpResponse). */
static bool bloom_http_read_chunked(
    bloom_http_reader *r,
    bloom_http_buf *body,
    bloom_str *err
) {
    for (;;) {
        const char *line = NULL;
        size_t len = 0;
        if (!bloom_http_reader_line(r, &line, &len, err)) {
            return false;
        }
        size_t hex_len = 0;
        while (hex_len < len && line[hex_len] != ';') {
            hex_len++;
        }
        while (hex_len > 0 && (line[hex_len - 1] == ' ' || line[hex_len - 1] == '\t')) {
            hex_len--;
        }
        if (hex_len == 0 || hex_len > 16) {
            *err = bloom_http_msg(r->what, "malformed chunk size");
            return false;
        }
        uint64_t size = 0;
        for (size_t i = 0; i < hex_len; i++) {
            char c = line[i];
            uint64_t d;
            if (c >= '0' && c <= '9') {
                d = (uint64_t)(c - '0');
            } else if (c >= 'a' && c <= 'f') {
                d = (uint64_t)(c - 'a' + 10);
            } else if (c >= 'A' && c <= 'F') {
                d = (uint64_t)(c - 'A' + 10);
            } else {
                *err = bloom_http_msg(r->what, "invalid hexadecimal chunk size");
                return false;
            }
            size = size * 16 + d;
            if (size > (uint64_t)BLOOM_HTTP_MAX_BODY) {
                *err = bloom_http_msg(r->what, "chunk exceeds the 4194304 byte body limit");
                return false;
            }
        }
        if ((uint64_t)body->len + size > (uint64_t)BLOOM_HTTP_MAX_BODY) {
            *err = bloom_http_msg(r->what, "body exceeds 4194304 bytes");
            return false;
        }
        if (size == 0) {
            break;
        }
        if (!bloom_http_reader_exact(r, (size_t)size, body, err)) {
            return false;
        }
        const char *term = NULL;
        size_t term_len = 0;
        if (!bloom_http_reader_line(r, &term, &term_len, err)) {
            return false;
        }
        if (term_len != 0) {
            *err = bloom_http_msg(r->what, "missing CRLF after chunk data");
            return false;
        }
    }
    /* Trailer section: validate and discard. */
    int trailers = 0;
    for (;;) {
        const char *line = NULL;
        size_t len = 0;
        if (!bloom_http_reader_line(r, &line, &len, err)) {
            return false;
        }
        if (len == 0) {
            break;
        }
        if (++trailers > BLOOM_HTTP_MAX_TRAILERS) {
            *err = bloom_http_msg(r->what, "too many trailer fields");
            return false;
        }
        size_t colon = 0;
        while (colon < len && line[colon] != ':') {
            colon++;
        }
        if (colon == 0 || colon >= len || !bloom_http_valid_token(line, colon)) {
            *err = bloom_http_msg(r->what, "malformed trailer field");
            return false;
        }
    }
    return true;
}

/* ---- URL parsing ------------------------------------------------------- */

typedef struct {
    bool tls;
    char host[512];
    size_t host_len;
    int64_t port;
    char path[2048];
    size_t path_len;
} bloom_http_url;

static bool bloom_http_parse_url(bloom_str url, bloom_http_url *out, bloom_str *err) {
    const char *u = url.data;
    size_t n = url.len;
    size_t i = 0;
    if (n >= 8 && memcmp(u, "https://", 8) == 0) {
        out->tls = true;
        i = 8;
    } else if (n >= 7 && memcmp(u, "http://", 7) == 0) {
        out->tls = false;
        i = 7;
    } else {
        *err = bloom_http_msg("http", "URL must start with `http://` or `https://`");
        return false;
    }
    size_t host_start = i;
    while (i < n && u[i] != '/' && u[i] != ':' && u[i] != '?' && u[i] != '#') {
        i++;
    }
    size_t host_len = i - host_start;
    if (host_len == 0 || host_len >= sizeof out->host) {
        *err = bloom_http_msg("http", "empty or too long host in URL");
        return false;
    }
    memcpy(out->host, u + host_start, host_len);
    out->host[host_len] = '\0';
    out->host_len = host_len;
    out->port = out->tls ? 443 : 80;
    if (i < n && u[i] == ':') {
        i++;
        size_t port_start = i;
        int64_t port = 0;
        while (i < n && u[i] >= '0' && u[i] <= '9') {
            port = port * 10 + (u[i] - '0');
            if (port > 65535) {
                *err = bloom_http_msg("http", "port out of range in URL");
                return false;
            }
            i++;
        }
        if (i == port_start) {
            *err = bloom_http_msg("http", "missing port digits in URL");
            return false;
        }
        out->port = port;
    }
    if (i >= n) {
        out->path[0] = '/';
        out->path_len = 1;
    } else if (u[i] == '?') {
        size_t q_end = i;
        while (q_end < n && u[q_end] != '#') {
            q_end++;
        }
        size_t query_len = q_end - i;
        if (1 + query_len >= sizeof out->path) {
            *err = bloom_http_msg("http", "path too long in URL");
            return false;
        }
        out->path[0] = '/';
        memcpy(out->path + 1, u + i, query_len);
        out->path_len = 1 + query_len;
    } else {
        size_t p_end = i;
        while (p_end < n && u[p_end] != '#') {
            p_end++;
        }
        size_t path_len = p_end - i;
        if (path_len >= sizeof out->path) {
            *err = bloom_http_msg("http", "path too long in URL");
            return false;
        }
        memcpy(out->path, u + i, path_len);
        out->path_len = path_len;
    }
    out->path[out->path_len] = '\0';
    return true;
}

/* ---- response reading -------------------------------------------------- */

static bool bloom_http_validate_header_block(bloom_str headers, bloom_str *err) {
    if (headers.len == 0) {
        return true;
    }
    bloom_http_header parsed[128];
    size_t len = headers.len;
    while (len > 0 && (headers.data[len - 1] == '\n' || headers.data[len - 1] == '\r')) {
        len--;
    }
    int count = bloom_http_parse_headers(headers.data, len, parsed, 128);
    if (count < 0) {
        *err = bloom_http_msg("http", "malformed request headers");
        return false;
    }
    return true;
}

/* Reads one complete response (after any interim responses). */
static bool bloom_http_read_response(
    bloom_http_reader *r,
    bool is_head,
    bloom_struct_std_http_HttpResponse *out,
    bloom_str *err
) {
    for (int interim = 0;; interim++) {
        /* Status line. Copied out of the reader buffer because subsequent
         * reads may reallocate it. */
        const char *line = NULL;
        size_t line_len = 0;
        if (!bloom_http_reader_line(r, &line, &line_len, err)) {
            return false;
        }
        char status_line[BLOOM_HTTP_MAX_LINE + 1];
        memcpy(status_line, line, line_len);
        status_line[line_len] = '\0';
        line = status_line;
        if (line_len < 12 || memcmp(line, "HTTP/1.", 7) != 0
            || (line[7] != '0' && line[7] != '1') || line[8] != ' ') {
            *err = bloom_http_msg("http", "malformed status line");
            return false;
        }
        if (line[9] < '0' || line[9] > '9' || line[10] < '0' || line[10] > '9'
            || line[11] < '0' || line[11] > '9') {
            *err = bloom_http_msg("http", "malformed status code");
            return false;
        }
        int status = (line[9] - '0') * 100 + (line[10] - '0') * 10 + (line[11] - '0');
        const char *reason = "";
        size_t reason_len = 0;
        if (line_len > 13) {
            reason = line + 13;
            reason_len = line_len - 13;
        }
        /* Header block. */
        bloom_http_buf block;
        bloom_http_buf_init(&block);
        for (;;) {
            const char *h = NULL;
            size_t h_len = 0;
            if (!bloom_http_reader_line(r, &h, &h_len, err)) {
                bloom_http_buf_free(&block);
                return false;
            }
            if (h_len == 0) {
                break;
            }
            if (block.len + h_len + 2 > BLOOM_HTTP_MAX_HEADERS) {
                bloom_http_buf_free(&block);
                *err = bloom_http_msg("http", "header block exceeds 16384 bytes");
                return false;
            }
            bloom_http_buf_append(&block, h, h_len);
            bloom_http_buf_append(&block, "\r\n", 2);
        }
        bloom_http_header headers[128];
        int count = bloom_http_parse_headers(block.data ? block.data : "", block.len, headers, 128);
        if (count < 0) {
            bloom_http_buf_free(&block);
            *err = bloom_http_msg("http", "malformed response headers");
            return false;
        }
        if (status >= 100 && status < 200 && status != 101) {
            /* Interim response: discard and read the final response. */
            bloom_http_buf_free(&block);
            if (interim + 1 >= BLOOM_HTTP_MAX_INTERIM) {
                *err = bloom_http_msg("http", "too many interim (1xx) responses");
                return false;
            }
            continue;
        }
        if (status == 101) {
            bloom_http_buf_free(&block);
            *err = bloom_http_msg("http", "switching protocols is not supported");
            return false;
        }
        int te_count = bloom_http_header_count(headers, count, "Transfer-Encoding");
        int cl_count = bloom_http_header_count(headers, count, "Content-Length");
        if (te_count > 0 && cl_count > 0) {
            bloom_http_buf_free(&block);
            *err = bloom_http_msg(
                "http",
                "ambiguous message framing: both Transfer-Encoding and Content-Length are present"
            );
            return false;
        }
        if (cl_count > 1) {
            bloom_http_buf_free(&block);
            *err = bloom_http_msg("http", "duplicate Content-Length in response");
            return false;
        }
        const char *te_value = NULL;
        size_t te_len = 0;
        int te = bloom_http_transfer_encoding(headers, count, &te_value, &te_len);
        if (te < 0) {
            bloom_http_buf_free(&block);
            *err = bloom_http_msg("http", "unsupported or ambiguous Transfer-Encoding");
            return false;
        }
        const char *cl_value = NULL;
        size_t cl_len = 0;
        if (cl_count == 1) {
            bloom_http_header_find(headers, count, "Content-Length", &cl_value, &cl_len);
        }
        bool no_body = is_head || status == 204 || status == 304;
        bloom_http_buf body;
        bloom_http_buf_init(&body);
        bool ok = true;
        if (no_body) {
            /* The protocol forbids a body; do not read one. */
        } else if (te == 1) {
            ok = bloom_http_read_chunked(r, &body, err);
        } else if (cl_count == 1) {
            int64_t length = 0;
            if (!bloom_http_parse_length(cl_value, cl_len, &length)) {
                *err = bloom_http_msg("http", "malformed Content-Length in response");
                ok = false;
            } else if (length > BLOOM_HTTP_MAX_BODY) {
                *err = bloom_http_msg("http", "response body exceeds 4194304 bytes");
                ok = false;
            } else {
                ok = bloom_http_reader_exact(r, (size_t)length, &body, err);
            }
        } else {
            ok = bloom_http_reader_to_eof(r, &body, err);
        }
        if (!ok) {
            bloom_http_buf_free(&body);
            bloom_http_buf_free(&block);
            return false;
        }
        out->f_status = status;
        out->f_reason = bloom_str_from_bytes(reason, reason_len);
        out->f_version = bloom_str_from_bytes(line, 8);
        out->f_headers = bloom_str_from_bytes(block.data ? block.data : "", block.len);
        out->f_body = bloom_str_from_bytes(body.data ? body.data : "", body.len);
        bloom_http_buf_free(&body);
        bloom_http_buf_free(&block);
        return true;
    }
}

/* ---- client ------------------------------------------------------------ */

static bloom_enum_Result_std_http_HttpResponse_str bloom_http_request_impl(
    bloom_str method,
    bloom_str url,
    bloom_str headers,
    bloom_str body,
    bloom_str ca_file
) {
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    if (!bloom_http_valid_token(method.data, method.len)) {
        err = bloom_http_msg("http", "invalid HTTP method");
        goto fail;
    }
    if (!bloom_http_validate_header_block(headers, &err)) {
        goto fail;
    }
    {
        bloom_http_url parsed;
        if (!bloom_http_parse_url(url, &parsed, &err)) {
            goto fail;
        }
        bloom_http_conn conn;
        conn.kind = parsed.tls ? 1 : 0;
        bloom_str host_str = bloom_str_from_bytes(parsed.host, parsed.host_len);
        if (parsed.tls) {
            bloom_enum_Result_i64_str connected = ca_file.len > 0
                ? bloom_tls_connect_with_ca(host_str, parsed.port, ca_file)
                : bloom_tls_connect(host_str, parsed.port);
            bloom_str_drop(&host_str);
            if (connected.tag == 1) {
                err = connected.data.m_Err._0;
                goto fail;
            }
            conn.handle = connected.data.m_Ok._0;
        } else {
            bloom_enum_Result_i64_str connected =
                bloom_net_tcp_connect(host_str, parsed.port);
            bloom_str_drop(&host_str);
            if (connected.tag == 1) {
                err = connected.data.m_Err._0;
                goto fail;
            }
            conn.handle = connected.data.m_Ok._0;
        }
        bloom_http_conn_set_timeout(conn, BLOOM_HTTP_CLIENT_TIMEOUT_MS);
        /* Build the request. */
        bloom_http_buf req;
        bloom_http_buf_init(&req);
        bloom_http_buf_append(&req, method.data, method.len);
        bloom_http_buf_append(&req, " ", 1);
        bloom_http_buf_append(&req, parsed.path, parsed.path_len);
        bloom_http_buf_append(&req, " HTTP/1.1\r\nHost: ", 17);
        bloom_http_buf_append(&req, parsed.host, parsed.host_len);
        if (parsed.port != (parsed.tls ? 443 : 80)) {
            char portbuf[16];
            snprintf(portbuf, sizeof portbuf, ":%" PRId64, parsed.port);
            bloom_http_buf_append(&req, portbuf, strlen(portbuf));
        }
        bloom_http_buf_append(&req, "\r\n", 2);
        if (headers.len > 0) {
            bloom_http_buf_append(&req, headers.data, headers.len);
            if (headers.data[headers.len - 1] != '\n') {
                bloom_http_buf_append(&req, "\r\n", 2);
            }
        }
        char lenbuf[32];
        snprintf(lenbuf, sizeof lenbuf, "Content-Length: %zu\r\n", body.len);
        bloom_http_buf_append(&req, lenbuf, strlen(lenbuf));
        bloom_http_buf_append(&req, "Connection: close\r\n\r\n", 21);
        if (body.len > 0) {
            bloom_http_buf_append(&req, body.data, body.len);
        }
        bloom_str request_text = bloom_str_from_bytes(req.data, req.len);
        bloom_enum_Result_i64_str written = bloom_http_conn_write(conn, request_text);
        bloom_str_drop(&request_text);
        bloom_http_buf_free(&req);
        if (written.tag == 1) {
            err = written.data.m_Err._0;
            bloom_http_conn_close(conn);
            goto fail;
        }
        /* Read the response. */
        bloom_http_reader reader;
        bloom_http_reader_init(&reader, conn, "http");
        bool is_head = bloom_http_eq_ci(method.data, method.len, "HEAD", 4);
        bloom_struct_std_http_HttpResponse out;
        bool ok = bloom_http_read_response(&reader, is_head, &out, &err);
        bloom_http_reader_free(&reader);
        bloom_http_conn_close(conn);
        if (!ok) {
            goto fail;
        }
        bloom_enum_Result_std_http_HttpResponse_str result;
        result.tag = 0;
        result.data.m_Ok._0 = out;
        return result;
    }
fail: {
    bloom_enum_Result_std_http_HttpResponse_str result;
    result.tag = 1;
    result.data.m_Err._0 = err;
    return result;
}
}

bloom_enum_Result_std_http_HttpResponse_str bloom_http_get(bloom_str url) {
    bloom_str method = bloom_str_from_bytes("GET", 3);
    bloom_str empty;
    empty.rc = 0;
    empty.data = NULL;
    empty.len = 0;
    bloom_enum_Result_std_http_HttpResponse_str result =
        bloom_http_request_impl(method, url, empty, empty, empty);
    bloom_str_drop(&method);
    return result;
}

bloom_enum_Result_std_http_HttpResponse_str bloom_http_post(
    bloom_str url,
    bloom_str content_type,
    bloom_str body
) {
    bloom_str method = bloom_str_from_bytes("POST", 4);
    bloom_str empty;
    empty.rc = 0;
    empty.data = NULL;
    empty.len = 0;
    bloom_str headers;
    headers.rc = 0;
    headers.data = NULL;
    headers.len = 0;
    if (content_type.len > 0) {
        bloom_http_buf h;
        bloom_http_buf_init(&h);
        bloom_http_buf_append(&h, "Content-Type: ", 14);
        bloom_http_buf_append(&h, content_type.data, content_type.len);
        headers = bloom_str_from_bytes(h.data, h.len);
        bloom_http_buf_free(&h);
    }
    bloom_enum_Result_std_http_HttpResponse_str result =
        bloom_http_request_impl(method, url, headers, body, empty);
    bloom_str_drop(&method);
    bloom_str_drop(&headers);
    return result;
}

bloom_enum_Result_std_http_HttpResponse_str bloom_http_request(
    bloom_str method,
    bloom_str url,
    bloom_str headers,
    bloom_str body
) {
    bloom_str empty;
    empty.rc = 0;
    empty.data = NULL;
    empty.len = 0;
    return bloom_http_request_impl(method, url, headers, body, empty);
}

bloom_enum_Result_std_http_HttpResponse_str bloom_http_get_with_ca(
    bloom_str url,
    bloom_str ca_file
) {
    bloom_str method = bloom_str_from_bytes("GET", 3);
    bloom_str empty;
    empty.rc = 0;
    empty.data = NULL;
    empty.len = 0;
    bloom_enum_Result_std_http_HttpResponse_str result =
        bloom_http_request_impl(method, url, empty, empty, ca_file);
    bloom_str_drop(&method);
    return result;
}

bloom_enum_Result_std_http_HttpResponse_str bloom_http_post_with_ca(
    bloom_str url,
    bloom_str content_type,
    bloom_str body,
    bloom_str ca_file
) {
    bloom_str method = bloom_str_from_bytes("POST", 4);
    bloom_str headers;
    headers.rc = 0;
    headers.data = NULL;
    headers.len = 0;
    if (content_type.len > 0) {
        bloom_http_buf h;
        bloom_http_buf_init(&h);
        bloom_http_buf_append(&h, "Content-Type: ", 14);
        bloom_http_buf_append(&h, content_type.data, content_type.len);
        headers = bloom_str_from_bytes(h.data, h.len);
        bloom_http_buf_free(&h);
    }
    bloom_enum_Result_std_http_HttpResponse_str result =
        bloom_http_request_impl(method, url, headers, body, ca_file);
    bloom_str_drop(&method);
    bloom_str_drop(&headers);
    return result;
}

bloom_enum_Result_std_http_HttpResponse_str bloom_http_request_with_ca(
    bloom_str method,
    bloom_str url,
    bloom_str headers,
    bloom_str body,
    bloom_str ca_file
) {
    return bloom_http_request_impl(method, url, headers, body, ca_file);
}

/* ---- server ------------------------------------------------------------ */

bloom_enum_Result_std_http_HttpRequest_str bloom_http_read_request(int64_t connection) {
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    bloom_http_conn conn;
    conn.kind = 0;
    conn.handle = connection;
    bloom_http_reader reader;
    bloom_http_reader_init(&reader, conn, "read_request");
    bloom_enum_Result_std_http_HttpRequest_str result;
    result.tag = 1;
    const char *line = NULL;
    size_t line_len = 0;
    if (!bloom_http_reader_line(&reader, &line, &line_len, &err)) {
        goto done;
    }
    /* Copied out of the reader buffer because subsequent reads may
     * reallocate it. */
    char request_line[BLOOM_HTTP_MAX_LINE + 1];
    memcpy(request_line, line, line_len);
    request_line[line_len] = '\0';
    line = request_line;
    size_t sp1 = 0;
    while (sp1 < line_len && line[sp1] != ' ') {
        sp1++;
    }
    size_t sp2 = sp1 + 1;
    while (sp2 < line_len && line[sp2] != ' ') {
        sp2++;
    }
    if (sp1 == 0 || sp1 >= line_len || sp2 >= line_len) {
        err = bloom_http_msg("read_request", "malformed request line");
        goto done;
    }
    const char *method = line;
    size_t method_len = sp1;
    const char *target = line + sp1 + 1;
    size_t target_len = sp2 - sp1 - 1;
    const char *version = line + sp2 + 1;
    size_t version_len = line_len - sp2 - 1;
    if (!bloom_http_valid_token(method, method_len)) {
        err = bloom_http_msg("read_request", "invalid method token");
        goto done;
    }
    if (target_len == 0) {
        err = bloom_http_msg("read_request", "empty request target");
        goto done;
    }
    bool http11 = version_len == 8 && memcmp(version, "HTTP/1.1", 8) == 0;
    bool http10 = version_len == 8 && memcmp(version, "HTTP/1.0", 8) == 0;
    if (!http11 && !http10) {
        err = bloom_http_msg("read_request", "unsupported HTTP version (expected 1.0 or 1.1)");
        goto done;
    }
    /* Header block. */
    bloom_http_buf block;
    bloom_http_buf_init(&block);
    for (;;) {
        const char *h = NULL;
        size_t h_len = 0;
        if (!bloom_http_reader_line(&reader, &h, &h_len, &err)) {
            bloom_http_buf_free(&block);
            goto done;
        }
        if (h_len == 0) {
            break;
        }
        if (block.len + h_len + 2 > BLOOM_HTTP_MAX_HEADERS) {
            bloom_http_buf_free(&block);
            err = bloom_http_msg("read_request", "header block exceeds 16384 bytes");
            goto done;
        }
        bloom_http_buf_append(&block, h, h_len);
        bloom_http_buf_append(&block, "\r\n", 2);
    }
    bloom_http_header headers[128];
    int count = bloom_http_parse_headers(block.data ? block.data : "", block.len, headers, 128);
    if (count < 0) {
        bloom_http_buf_free(&block);
        err = bloom_http_msg("read_request", "malformed headers");
        goto done;
    }
    if (http11 && !bloom_http_header_find(headers, count, "Host", NULL, NULL)) {
        bloom_http_buf_free(&block);
        err = bloom_http_msg("read_request", "HTTP/1.1 request without Host header");
        goto done;
    }
    int te_count = bloom_http_header_count(headers, count, "Transfer-Encoding");
    int cl_count = bloom_http_header_count(headers, count, "Content-Length");
    if (te_count > 0 && cl_count > 0) {
        bloom_http_buf_free(&block);
        err = bloom_http_msg(
            "read_request",
            "ambiguous message framing: both Transfer-Encoding and Content-Length are present"
        );
        goto done;
    }
    if (cl_count > 1) {
        bloom_http_buf_free(&block);
        err = bloom_http_msg("read_request", "duplicate Content-Length header");
        goto done;
    }
    const char *te_value = NULL;
    size_t te_len = 0;
    int te = bloom_http_transfer_encoding(headers, count, &te_value, &te_len);
    if (te < 0) {
        bloom_http_buf_free(&block);
        err = bloom_http_msg("read_request", "unsupported or ambiguous Transfer-Encoding");
        goto done;
    }
    bloom_http_buf body;
    bloom_http_buf_init(&body);
    bool ok = true;
    if (te == 1) {
        ok = bloom_http_read_chunked(&reader, &body, &err);
    } else if (cl_count == 1) {
        const char *cl_value = NULL;
        size_t cl_len = 0;
        bloom_http_header_find(headers, count, "Content-Length", &cl_value, &cl_len);
        int64_t body_len = 0;
        if (!bloom_http_parse_length(cl_value, cl_len, &body_len)) {
            err = bloom_http_msg("read_request", "malformed Content-Length header");
            ok = false;
        } else if (body_len > BLOOM_HTTP_MAX_BODY) {
            err = bloom_http_msg("read_request", "body exceeds 4194304 bytes");
            ok = false;
        } else {
            ok = bloom_http_reader_exact(&reader, (size_t)body_len, &body, &err);
        }
    }
    if (!ok) {
        bloom_http_buf_free(&body);
        bloom_http_buf_free(&block);
        goto done;
    }
    {
        bloom_struct_std_http_HttpRequest out;
        out.f_method = bloom_str_from_bytes(method, method_len);
        out.f_target = bloom_str_from_bytes(target, target_len);
        out.f_version = bloom_str_from_bytes(version, version_len);
        out.f_headers = bloom_str_from_bytes(block.data ? block.data : "", block.len);
        out.f_body = bloom_str_from_bytes(body.data ? body.data : "", body.len);
        result.tag = 0;
        result.data.m_Ok._0 = out;
    }
    bloom_http_buf_free(&body);
    bloom_http_buf_free(&block);
done:
    bloom_http_reader_free(&reader);
    if (result.tag == 1 && err.data != NULL) {
        result.data.m_Err._0 = err;
    }
    return result;
}

static const char *bloom_http_reason(int status) {
    switch (status) {
        case 200: return "OK";
        case 201: return "Created";
        case 204: return "No Content";
        case 301: return "Moved Permanently";
        case 302: return "Found";
        case 304: return "Not Modified";
        case 400: return "Bad Request";
        case 403: return "Forbidden";
        case 404: return "Not Found";
        case 405: return "Method Not Allowed";
        case 411: return "Length Required";
        case 413: return "Payload Too Large";
        case 415: return "Unsupported Media Type";
        case 431: return "Request Header Fields Too Large";
        case 503: return "Service Unavailable";
        case 500: return "Internal Server Error";
        case 501: return "Not Implemented";
        case 505: return "HTTP Version Not Supported";
        default: return "Unknown";
    }
}

static bloom_enum_Result_bool_str bloom_http_write_response_impl(
    int64_t connection,
    int64_t status,
    bloom_str content_type,
    bloom_str headers,
    const void *body,
    size_t body_len,
    bool suppress_body,
    const char *op
) {
    if (status < 100 || status > 599) {
        return bloom_http_err_bool(bloom_http_msg(op, "status out of range (100..599)"));
    }
    for (size_t i = 0; i < content_type.len; i++) {
        if (content_type.data[i] == '\r' || content_type.data[i] == '\n') {
            return bloom_http_err_bool(bloom_http_msg(
                op,
                "content type must not contain CR or LF"
            ));
        }
    }
    /* Extra headers: a raw `Name: value` block, validated and re-emitted.
     * Framing headers stay under the library's control. */
    bloom_http_header parsed[128];
    int count = 0;
    if (headers.len > 0) {
        if (headers.len > BLOOM_HTTP_MAX_HEADERS) {
            return bloom_http_err_bool(bloom_http_msg(
                op,
                "extra headers exceed 16384 bytes"
            ));
        }
        count = bloom_http_parse_headers(headers.data, headers.len, parsed, 128);
        if (count < 0) {
            return bloom_http_err_bool(bloom_http_msg(
                op,
                "malformed extra headers (expected `Name: value` lines)"
            ));
        }
        for (int i = 0; i < count; i++) {
            const bloom_http_header *h = &parsed[i];
            if (bloom_http_eq_ci(h->name, h->name_len, "Content-Length", 14)
                || bloom_http_eq_ci(h->name, h->name_len, "Connection", 10)
                || bloom_http_eq_ci(h->name, h->name_len, "Transfer-Encoding", 17)) {
                return bloom_http_err_bool(bloom_http_msg(
                    op,
                    "`Content-Length`, `Connection` and `Transfer-Encoding` are managed by the library"
                ));
            }
            for (size_t k = 0; k < h->value_len; k++) {
                if (h->value[k] == '\r' || h->value[k] == '\n') {
                    return bloom_http_err_bool(bloom_http_msg(
                        op,
                        "header values must not contain CR or LF"
                    ));
                }
            }
        }
    }
    /* 204 and 304 responses never carry a body (RFC 9110). */
    bool no_body = status == 204 || status == 304;
    bloom_http_conn conn;
    conn.kind = 0;
    conn.handle = connection;
    bloom_http_buf out;
    bloom_http_buf_init(&out);
    char head[64];
    snprintf(
        head,
        sizeof head,
        "HTTP/1.1 %" PRId64 " %s\r\n",
        status,
        bloom_http_reason((int)status)
    );
    bloom_http_buf_append(&out, head, strlen(head));
    if (content_type.len > 0) {
        bloom_http_buf_append(&out, "Content-Type: ", 14);
        bloom_http_buf_append(&out, content_type.data, content_type.len);
        bloom_http_buf_append(&out, "\r\n", 2);
    }
    for (int i = 0; i < count; i++) {
        bloom_http_buf_append(&out, parsed[i].name, parsed[i].name_len);
        bloom_http_buf_append(&out, ": ", 2);
        bloom_http_buf_append(&out, parsed[i].value, parsed[i].value_len);
        bloom_http_buf_append(&out, "\r\n", 2);
    }
    char lenbuf[48];
    snprintf(lenbuf, sizeof lenbuf, "Content-Length: %zu\r\n", no_body ? (size_t)0 : body_len);
    bloom_http_buf_append(&out, lenbuf, strlen(lenbuf));
    bloom_http_buf_append(&out, "Connection: close\r\n\r\n", 21);
    if (!no_body && !suppress_body && body_len > 0 && body != NULL) {
        bloom_http_buf_append(&out, (const char *)body, body_len);
    }
    bloom_str message = bloom_str_from_bytes(out.data, out.len);
    bloom_http_buf_free(&out);
    bloom_enum_Result_i64_str written = bloom_http_conn_write(conn, message);
    bloom_str_drop(&message);
    if (written.tag == 1) {
        return bloom_http_err_bool(written.data.m_Err._0);
    }
    return bloom_http_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_http_write_response(
    int64_t connection,
    int64_t status,
    bloom_str content_type,
    bloom_str body
) {
    bloom_str empty;
    empty.rc = 0;
    empty.data = NULL;
    empty.len = 0;
    return bloom_http_write_response_impl(
        connection,
        status,
        content_type,
        empty,
        body.data,
        body.len,
        false,
        "write_response"
    );
}

bloom_enum_Result_bool_str bloom_http_write_response_full(
    int64_t connection,
    int64_t status,
    bloom_str content_type,
    bloom_str headers,
    bloom_str body
) {
    return bloom_http_write_response_impl(
        connection,
        status,
        content_type,
        headers,
        body.data,
        body.len,
        false,
        "write_response_full"
    );
}

/* Binary-safe responses: the body is a byte buffer with its exact length;
 * embedded NUL bytes are preserved. `_head` sends the same headers
 * (including the real Content-Length) without the body. */
bloom_enum_Result_bool_str bloom_http_write_response_bytes(
    int64_t connection,
    int64_t status,
    bloom_str content_type,
    bloom_str headers,
    bloom_struct_std_fs_Bytes body
) {
    bloom_fs_bytes *b = bloom_fs_bytes_unwrap(body);
    if (b == NULL) {
        return bloom_http_err_bool(bloom_http_msg("write_response_bytes", "invalid byte buffer"));
    }
    return bloom_http_write_response_impl(
        connection,
        status,
        content_type,
        headers,
        b->data,
        b->len,
        false,
        "write_response_bytes"
    );
}

bloom_enum_Result_bool_str bloom_http_write_response_bytes_head(
    int64_t connection,
    int64_t status,
    bloom_str content_type,
    bloom_str headers,
    bloom_struct_std_fs_Bytes body
) {
    bloom_fs_bytes *b = bloom_fs_bytes_unwrap(body);
    if (b == NULL) {
        return bloom_http_err_bool(bloom_http_msg(
            "write_response_bytes_head",
            "invalid byte buffer"
        ));
    }
    return bloom_http_write_response_impl(
        connection,
        status,
        content_type,
        headers,
        b->data,
        b->len,
        true,
        "write_response_bytes_head"
    );
}

bloom_enum_Option_str bloom_http_header_get(bloom_str headers, bloom_str name) {
    bloom_enum_Option_str result;
    bloom_http_header parsed[128];
    if (name.len == 0 || name.len > 256) {
        result.tag = 1;
        return result;
    }
    char namebuf[257];
    memcpy(namebuf, name.data, name.len);
    namebuf[name.len] = '\0';
    int count = bloom_http_parse_headers(headers.data, headers.len, parsed, 128);
    if (count < 0) {
        result.tag = 1;
        return result;
    }
    const char *value = NULL;
    size_t value_len = 0;
    if (!bloom_http_header_find(parsed, count, namebuf, &value, &value_len)) {
        result.tag = 1;
        return result;
    }
    result.tag = 0;
    result.data.m_Some._0 = bloom_str_from_bytes(value, value_len);
    return result;
}
