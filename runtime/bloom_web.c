/* Bloom std::web runtime — pragmatic routing for REST APIs.
 *
 * The router is a small, synchronous data structure: applications register
 * routes (method, path pattern, application name), call `resolve` with the
 * request method and target, and branch on the returned match. There are no
 * callbacks — Bloom has no closures — so the application dispatches on
 * `Match.name` itself (for example with a `match` on the name).
 *
 * Pattern syntax: segments separated by `/`; `:name` captures one segment.
 * Trailing slashes are ignored; empty segments in the middle are rejected.
 * `resolve` splits the target on `/` *first* and percent-decodes each
 * segment *afterwards*, so encoded separators (`%2F`) never create new
 * routes. Query parameters use `&`/`=` with form-style decoding
 * (`+` is a space); the first occurrence of a repeated key wins. Path
 * parameters take precedence over query parameters with the same name.
 *
 * Matching: a request path is matched per method. Among all matching
 * routes the most specific one wins (a fixed segment beats a `:param` at
 * the first differing position). Registration rejects duplicate patterns,
 * duplicate names per method and patterns with the same shape (the same
 * fixed/param structure), so the winner is always unambiguous.
 *
 * Status semantics of `resolve`:
 *   200  a route matched (`name` and `params` are filled)
 *   404  no route matches the path (`name` and `allow` are empty)
 *   405  the path matches routes of other methods (`allow` lists them)
 * `Err` is reserved for malformed input: invalid methods, invalid percent
 * encoding, decoded bytes that are not valid UTF-8, or limit violations.
 *
 * Routing is not authorization: applications must implement authentication
 * and authorization themselves; a 200 from `resolve` only means that a
 * route pattern matched.
 *
 * Limits (all enforced with a clear error): 4096 routes, pattern 4096
 * bytes, route name 256 bytes, request target 65536 bytes, at most 256
 * path segments and 256 query parameters per resolve.
 */

#define BLOOM_WEB_MAX_ROUTES 4096
#define BLOOM_WEB_MAX_PATTERN 4096
#define BLOOM_WEB_MAX_NAME 256
#define BLOOM_WEB_MAX_METHOD 16
#define BLOOM_WEB_MAX_TARGET (64 * 1024)
#define BLOOM_WEB_MAX_SEGMENTS 256
#define BLOOM_WEB_MAX_QUERY 256
#define BLOOM_WEB_MAX_PARAM_NAME 64

typedef struct {
    bool is_param;
    const char *text; /* fixed literal, or parameter name (no ':') */
    size_t len;
} bloom_web_seg;

typedef struct {
    char *method; /* uppercased */
    size_t method_len;
    char *pattern; /* normalized (no trailing slash) */
    size_t pattern_len;
    char *name;
    size_t name_len;
    bloom_web_seg *segs;
    size_t seg_count;
} bloom_web_route_def;

typedef struct {
    size_t rc;
    bloom_web_route_def *routes;
    size_t count;
    size_t cap;
} bloom_web_router;

/* ---- small helpers ------------------------------------------------------ */

static bloom_str bloom_web_msg(const char *op, const char *detail) {
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

static bool bloom_web_utf8_valid(const unsigned char *s, size_t n) {
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

static bloom_enum_Result_bool_str bloom_web_ok_bool(bool v) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_bool_str bloom_web_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_struct_std_web_Router bloom_web_wrap(bloom_web_router *rt) {
    bloom_struct_std_web_Router v;
    v.f_id = (bloom_web_handle)(intptr_t)rt;
    return v;
}

static bloom_web_router *bloom_web_unwrap(bloom_struct_std_web_Router v) {
    return (bloom_web_router *)(intptr_t)v.f_id;
}

bloom_web_handle bloom_web_retain(bloom_web_handle h) {
    bloom_web_router *rt = (bloom_web_router *)(intptr_t)h;
    if (rt != NULL) {
        rt->rc++;
    }
    return h;
}

static void bloom_web_route_free(bloom_web_route_def *r) {
    free(r->method);
    free(r->pattern);
    free(r->name);
    free(r->segs);
}

void bloom_web_release(bloom_web_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_web_router *rt = (bloom_web_router *)(intptr_t)*h;
    *h = 0;
    if (rt->rc > 0) {
        rt->rc--;
    }
    if (rt->rc == 0) {
        for (size_t i = 0; i < rt->count; i++) {
            bloom_web_route_free(&rt->routes[i]);
        }
        free(rt->routes);
        free(rt);
    }
}

bloom_struct_std_web_Router bloom_web_new_router(void) {
    bloom_web_router *rt = (bloom_web_router *)calloc(1, sizeof(bloom_web_router));
    if (rt == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    rt->rc = 1;
    return bloom_web_wrap(rt);
}

/* ---- percent decoding --------------------------------------------------- */

/* Returns 0 on success, 1 for a bad `%` escape, 2 for invalid UTF-8. */
static int bloom_web_decode(
    const char *p,
    size_t n,
    bool plus_space,
    char **out,
    size_t *out_len
) {
    char *buf = (char *)malloc(n + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t at = 0;
    size_t i = 0;
    while (i < n) {
        char c = p[i];
        if (c == '%') {
            if (i + 2 >= n) {
                free(buf);
                return 1;
            }
            int hi = bloom_json_hex_digit(p[i + 1]);
            int lo = bloom_json_hex_digit(p[i + 2]);
            if (hi < 0 || lo < 0) {
                free(buf);
                return 1;
            }
            buf[at++] = (char)(hi * 16 + lo);
            i += 3;
            continue;
        }
        if (c == '+' && plus_space) {
            buf[at++] = ' ';
            i++;
            continue;
        }
        buf[at++] = c;
        i++;
    }
    if (!bloom_web_utf8_valid((const unsigned char *)buf, at)) {
        free(buf);
        return 2;
    }
    *out = buf;
    *out_len = at;
    return 0;
}

/* ---- method normalization ---------------------------------------------- */

static bool bloom_web_normalize_method(
    bloom_str method,
    char *out,
    size_t *out_len
) {
    if (method.len == 0 || method.len > BLOOM_WEB_MAX_METHOD) {
        return false;
    }
    for (size_t i = 0; i < method.len; i++) {
        char c = method.data[i];
        if (c >= 'a' && c <= 'z') {
            c = (char)(c - 32);
        } else if (!(c >= 'A' && c <= 'Z')) {
            return false;
        }
        out[i] = c;
    }
    out[method.len] = '\0';
    *out_len = method.len;
    return true;
}

/* ---- route registration ------------------------------------------------- */

static bool bloom_web_valid_param_name(const char *p, size_t n) {
    if (n == 0 || n > BLOOM_WEB_MAX_PARAM_NAME) {
        return false;
    }
    char c = p[0];
    if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_')) {
        return false;
    }
    for (size_t i = 1; i < n; i++) {
        c = p[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
              || (c >= '0' && c <= '9') || c == '_')) {
            return false;
        }
    }
    return true;
}

/* Builds the shape string used for ambiguity checks: identical shapes mean
 * the same match set (same fixed literals and parameter positions). Fixed
 * segments cannot contain control bytes, so `\x01` is a safe separator. */
static void bloom_web_shape(
    const bloom_web_seg *segs,
    size_t count,
    char *out,
    size_t out_cap
) {
    size_t at = 0;
    for (size_t i = 0; i < count && at + 2 < out_cap; i++) {
        if (segs[i].is_param) {
            out[at++] = 'P';
        } else {
            out[at++] = 'F';
            for (size_t k = 0; k < segs[i].len && at + 1 < out_cap; k++) {
                out[at++] = segs[i].text[k];
            }
        }
        out[at++] = '\x01';
    }
    out[at] = '\0';
}

bloom_enum_Result_bool_str bloom_web_route(
    bloom_struct_std_web_Router router,
    bloom_str method,
    bloom_str pattern,
    bloom_str name
) {
    bloom_web_router *rt = bloom_web_unwrap(router);
    if (rt == NULL) {
        return bloom_web_err_bool(bloom_web_msg("route", "invalid router"));
    }
    char method_buf[BLOOM_WEB_MAX_METHOD + 1];
    size_t method_len = 0;
    if (!bloom_web_normalize_method(method, method_buf, &method_len)) {
        return bloom_web_err_bool(bloom_web_msg(
            "route",
            "method must be 1..16 ASCII letters"
        ));
    }
    if (name.len == 0 || name.len > BLOOM_WEB_MAX_NAME) {
        return bloom_web_err_bool(bloom_web_msg(
            "route",
            "name must not be empty or longer than 256 bytes"
        ));
    }
    if (pattern.len == 0 || pattern.len > BLOOM_WEB_MAX_PATTERN) {
        return bloom_web_err_bool(bloom_web_msg(
            "route",
            "pattern must not be empty or longer than 4096 bytes"
        ));
    }
    if (pattern.data[0] != '/') {
        return bloom_web_err_bool(bloom_web_msg(
            "route",
            "pattern must start with `/`"
        ));
    }
    /* Strip trailing slashes; "/" stays "/". */
    size_t plen = pattern.len;
    while (plen > 1 && pattern.data[plen - 1] == '/') {
        plen--;
    }
    /* Parse segments. */
    bloom_web_seg segs[BLOOM_WEB_MAX_SEGMENTS];
    size_t seg_count = 0;
    size_t i = 0;
    while (i < plen) {
        if (pattern.data[i] != '/') {
            return bloom_web_err_bool(bloom_web_msg("route", "malformed pattern"));
        }
        i++;
        if (i >= plen) {
            break; /* trailing slash, already stripped */
        }
        size_t start = i;
        while (i < plen && pattern.data[i] != '/') {
            i++;
        }
        size_t seg_len = i - start;
        if (seg_len == 0) {
            return bloom_web_err_bool(bloom_web_msg(
                "route",
                "empty path segment in pattern"
            ));
        }
        if (seg_count >= BLOOM_WEB_MAX_SEGMENTS) {
            return bloom_web_err_bool(bloom_web_msg(
                "route",
                "too many pattern segments (max 256)"
            ));
        }
        const char *seg = pattern.data + start;
        if (seg[0] == ':') {
            size_t name_len = seg_len - 1;
            if (!bloom_web_valid_param_name(seg + 1, name_len)) {
                return bloom_web_err_bool(bloom_web_msg(
                    "route",
                    "invalid parameter name in pattern (expected `:name`)"
                ));
            }
            for (size_t k = 0; k < seg_count; k++) {
                if (segs[k].is_param && segs[k].len == name_len
                    && memcmp(segs[k].text, seg + 1, name_len) == 0) {
                    return bloom_web_err_bool(bloom_web_msg(
                        "route",
                        "duplicate parameter name in pattern"
                    ));
                }
            }
            segs[seg_count].is_param = true;
            segs[seg_count].text = seg + 1;
            segs[seg_count].len = name_len;
        } else {
            for (size_t k = 0; k < seg_len; k++) {
                char c = seg[k];
                if (c == ':' || c == '?' || c == '#' || c < 0x20) {
                    return bloom_web_err_bool(bloom_web_msg(
                        "route",
                        "invalid character in pattern segment (`:`, `?`, `#` and control bytes are reserved)"
                    ));
                }
            }
            segs[seg_count].is_param = false;
            segs[seg_count].text = seg;
            segs[seg_count].len = seg_len;
        }
        seg_count++;
    }
    char shape[BLOOM_WEB_MAX_PATTERN + BLOOM_WEB_MAX_SEGMENTS + 2];
    bloom_web_shape(segs, seg_count, shape, sizeof shape);
    /* Duplicate and ambiguity checks against existing routes of this method. */
    for (size_t r = 0; r < rt->count; r++) {
        bloom_web_route_def *existing = &rt->routes[r];
        if (existing->method_len != method_len
            || memcmp(existing->method, method_buf, method_len) != 0) {
            continue;
        }
        if (existing->pattern_len == plen
            && memcmp(existing->pattern, pattern.data, plen) == 0) {
            return bloom_web_err_bool(bloom_web_msg(
                "route",
                "this pattern is already registered for this method"
            ));
        }
        if (existing->name_len == name.len
            && memcmp(existing->name, name.data, name.len) == 0) {
            return bloom_web_err_bool(bloom_web_msg(
                "route",
                "this name is already used for a route of this method"
            ));
        }
        char existing_shape[BLOOM_WEB_MAX_PATTERN + BLOOM_WEB_MAX_SEGMENTS + 2];
        bloom_web_shape(existing->segs, existing->seg_count, existing_shape, sizeof existing_shape);
        if (strcmp(shape, existing_shape) == 0) {
            return bloom_web_err_bool(bloom_web_msg(
                "route",
                "ambiguous route: a route with the same shape is already registered for this method"
            ));
        }
    }
    if (rt->count >= BLOOM_WEB_MAX_ROUTES) {
        return bloom_web_err_bool(bloom_web_msg(
            "route",
            "too many routes (max 4096)"
        ));
    }
    /* Commit: copy the pattern and rebuild segments against the copy. */
    if (rt->count == rt->cap) {
        size_t cap = rt->cap ? rt->cap * 2 : 8;
        bloom_web_route_def *nd = (bloom_web_route_def *)realloc(
            rt->routes,
            cap * sizeof *nd
        );
        if (nd == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        rt->routes = nd;
        rt->cap = cap;
    }
    bloom_web_route_def *route = &rt->routes[rt->count];
    memset(route, 0, sizeof *route);
    route->method = (char *)malloc(method_len + 1);
    route->pattern = (char *)malloc(plen + 1);
    route->name = (char *)malloc(name.len + 1);
    route->segs = (bloom_web_seg *)malloc(
        (seg_count ? seg_count : 1) * sizeof(bloom_web_seg)
    );
    if (route->method == NULL || route->pattern == NULL || route->name == NULL
        || route->segs == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(route->method, method_buf, method_len + 1);
    route->method_len = method_len;
    memcpy(route->pattern, pattern.data, plen);
    route->pattern[plen] = '\0';
    route->pattern_len = plen;
    memcpy(route->name, name.data, name.len);
    route->name[name.len] = '\0';
    route->name_len = name.len;
    for (size_t k = 0; k < seg_count; k++) {
        route->segs[k].is_param = segs[k].is_param;
        route->segs[k].len = segs[k].len;
        route->segs[k].text = route->pattern + (segs[k].text - pattern.data);
    }
    route->seg_count = seg_count;
    rt->count++;
    return bloom_web_ok_bool(true);
}

/* ---- request target parsing --------------------------------------------- */

typedef struct {
    char *data;
    size_t len;
} bloom_web_owned;

typedef struct {
    bloom_web_owned *items;
    size_t count;
    size_t cap;
} bloom_web_list;

static void bloom_web_list_init(bloom_web_list *l) {
    l->items = NULL;
    l->count = 0;
    l->cap = 0;
}

static void bloom_web_list_push(bloom_web_list *l, char *data, size_t len) {
    if (l->count == l->cap) {
        size_t cap = l->cap ? l->cap * 2 : 8;
        bloom_web_owned *nd = (bloom_web_owned *)realloc(
            l->items,
            cap * sizeof *nd
        );
        if (nd == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        l->items = nd;
        l->cap = cap;
    }
    l->items[l->count].data = data;
    l->items[l->count].len = len;
    l->count++;
}

static void bloom_web_list_free(bloom_web_list *l) {
    for (size_t i = 0; i < l->count; i++) {
        free(l->items[i].data);
    }
    free(l->items);
    l->items = NULL;
    l->count = 0;
    l->cap = 0;
}

/* Splits the path part into decoded segments (empty segments collapse). */
static int bloom_web_split_path(
    const char *p,
    size_t n,
    bloom_web_list *out,
    bloom_str *err
) {
    size_t i = 0;
    while (i < n) {
        while (i < n && p[i] == '/') {
            i++;
        }
        if (i >= n) {
            break;
        }
        size_t start = i;
        while (i < n && p[i] != '/') {
            i++;
        }
        if (out->count >= BLOOM_WEB_MAX_SEGMENTS) {
            *err = bloom_web_msg("resolve", "too many path segments (max 256)");
            return -1;
        }
        char *decoded = NULL;
        size_t decoded_len = 0;
        int rc = bloom_web_decode(
            p + start,
            i - start,
            false,
            &decoded,
            &decoded_len
        );
        if (rc == 1) {
            *err = bloom_web_msg("resolve", "invalid percent-encoding in path");
            return -1;
        }
        if (rc == 2) {
            *err = bloom_web_msg("resolve", "invalid UTF-8 in decoded path segment");
            return -1;
        }
        bloom_web_list_push(out, decoded, decoded_len);
    }
    return 0;
}

/* Parses `key=value&...` with form-style decoding; first key wins. */
static int bloom_web_split_query(
    const char *p,
    size_t n,
    bloom_web_list *keys,
    bloom_web_list *values,
    bloom_str *err
) {
    size_t i = 0;
    while (i < n) {
        size_t start = i;
        while (i < n && p[i] != '&') {
            i++;
        }
        size_t part_len = i - start;
        if (i < n) {
            i++; /* skip '&' */
        }
        if (part_len == 0) {
            continue;
        }
        const char *part = p + start;
        size_t eq = 0;
        while (eq < part_len && part[eq] != '=') {
            eq++;
        }
        size_t key_raw = eq;
        size_t val_raw = eq < part_len ? part_len - eq - 1 : 0;
        const char *val_start = eq < part_len ? part + eq + 1 : part + part_len;
        char *key = NULL;
        size_t key_len = 0;
        char *value = NULL;
        size_t value_len = 0;
        int rc = bloom_web_decode(part, key_raw, true, &key, &key_len);
        if (rc == 1) {
            *err = bloom_web_msg("resolve", "invalid percent-encoding in query");
            return -1;
        }
        if (rc == 2) {
            *err = bloom_web_msg("resolve", "invalid UTF-8 in decoded query key");
            return -1;
        }
        rc = bloom_web_decode(val_start, val_raw, true, &value, &value_len);
        if (rc == 1) {
            free(key);
            *err = bloom_web_msg("resolve", "invalid percent-encoding in query");
            return -1;
        }
        if (rc == 2) {
            free(key);
            *err = bloom_web_msg("resolve", "invalid UTF-8 in decoded query value");
            return -1;
        }
        if (key_len == 0) {
            free(key);
            free(value);
            continue; /* empty keys are ignored */
        }
        /* First occurrence wins. */
        bool seen = false;
        for (size_t k = 0; k < keys->count && !seen; k++) {
            if (keys->items[k].len == key_len
                && memcmp(keys->items[k].data, key, key_len) == 0) {
                seen = true;
            }
        }
        if (seen) {
            free(key);
            free(value);
            continue;
        }
        if (keys->count >= BLOOM_WEB_MAX_QUERY) {
            free(key);
            free(value);
            *err = bloom_web_msg("resolve", "too many query parameters (max 256)");
            return -1;
        }
        bloom_web_list_push(keys, key, key_len);
        bloom_web_list_push(values, value, value_len);
    }
    return 0;
}

/* ---- matching ----------------------------------------------------------- */

static bool bloom_web_route_matches(
    const bloom_web_route_def *route,
    const bloom_web_list *segs
) {
    if (route->seg_count != segs->count) {
        return false;
    }
    for (size_t i = 0; i < route->seg_count; i++) {
        if (!route->segs[i].is_param) {
            if (route->segs[i].len != segs->items[i].len
                || memcmp(route->segs[i].text, segs->items[i].data, segs->items[i].len) != 0) {
                return false;
            }
        }
    }
    return true;
}

/* Returns true when `a` is more specific than `b` (fixed beats param at the
 * first differing position). Both must match the same path. */
static bool bloom_web_more_specific(
    const bloom_web_route_def *a,
    const bloom_web_route_def *b
) {
    for (size_t i = 0; i < a->seg_count && i < b->seg_count; i++) {
        if (a->segs[i].is_param == b->segs[i].is_param) {
            continue;
        }
        return !a->segs[i].is_param;
    }
    return false;
}

bloom_enum_Result_std_web_Match_str bloom_web_resolve(
    bloom_struct_std_web_Router router,
    bloom_str method,
    bloom_str target
) {
    bloom_enum_Result_std_web_Match_str result;
    bloom_web_router *rt = bloom_web_unwrap(router);
    if (rt == NULL) {
        result.tag = 1;
        result.data.m_Err._0 = bloom_web_msg("resolve", "invalid router");
        return result;
    }
    char method_buf[BLOOM_WEB_MAX_METHOD + 1];
    size_t method_len = 0;
    if (!bloom_web_normalize_method(method, method_buf, &method_len)) {
        result.tag = 1;
        result.data.m_Err._0 = bloom_web_msg(
            "resolve",
            "method must be 1..16 ASCII letters"
        );
        return result;
    }
    if (target.len > BLOOM_WEB_MAX_TARGET) {
        result.tag = 1;
        result.data.m_Err._0 = bloom_web_msg(
            "resolve",
            "target too long (max 65536 bytes)"
        );
        return result;
    }
    size_t qmark = target.len;
    for (size_t i = 0; i < target.len; i++) {
        if (target.data[i] == '?') {
            qmark = i;
            break;
        }
    }
    bloom_web_list segs;
    bloom_web_list qkeys;
    bloom_web_list qvalues;
    bloom_web_list_init(&segs);
    bloom_web_list_init(&qkeys);
    bloom_web_list_init(&qvalues);
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    if (bloom_web_split_path(target.data, qmark, &segs, &err) != 0) {
        bloom_web_list_free(&segs);
        result.tag = 1;
        result.data.m_Err._0 = err;
        return result;
    }
    if (qmark < target.len) {
        if (bloom_web_split_query(
                target.data + qmark + 1,
                target.len - qmark - 1,
                &qkeys,
                &qvalues,
                &err
            ) != 0) {
            bloom_web_list_free(&segs);
            bloom_web_list_free(&qkeys);
            bloom_web_list_free(&qvalues);
            result.tag = 1;
            result.data.m_Err._0 = err;
            return result;
        }
    }
    /* Find the most specific matching route for this method. */
    bloom_web_route_def *best = NULL;
    for (size_t r = 0; r < rt->count; r++) {
        bloom_web_route_def *route = &rt->routes[r];
        if (route->method_len != method_len
            || memcmp(route->method, method_buf, method_len) != 0) {
            continue;
        }
        if (!bloom_web_route_matches(route, &segs)) {
            continue;
        }
        if (best == NULL || bloom_web_more_specific(route, best)) {
            best = route;
        }
    }
    bloom_struct_std_web_Match m;
    memset(&m, 0, sizeof m);
    m.f_status = 0;
    m.f_name = bloom_str_from_bytes("", 0);
    m.f_allow = bloom_str_from_bytes("", 0);
    m.f_params = bloom_json_make_object();
    if (best != NULL) {
        m.f_status = 200;
        bloom_str_drop(&m.f_name);
        m.f_name = bloom_str_from_bytes(best->name, best->name_len);
        /* Path parameters first. */
        for (size_t i = 0; i < best->seg_count; i++) {
            if (!best->segs[i].is_param) {
                continue;
            }
            bloom_str key = bloom_str_from_bytes(
                best->segs[i].text,
                best->segs[i].len
            );
            bloom_str raw = bloom_str_from_bytes(
                segs.items[i].data,
                segs.items[i].len
            );
            bloom_struct_std_json_JsonValue value = bloom_json_make_string(raw);
            bloom_str_drop(&raw);
            bloom_enum_Result_bool_str set = bloom_json_set(m.f_params, key, value);
            if (set.tag == 1) {
                bloom_str_drop(&set.data.m_Err._0);
            }
            bloom_str_drop(&key);
            /* The temporary value is a managed handle: dropping it releases
             * the reference held by the temporary (set retained its own). */
            bloom_json_release(&value.f_id);
        }
        /* Query parameters that do not collide with path parameters. */
        for (size_t i = 0; i < qkeys.count; i++) {
            bloom_str key = bloom_str_from_bytes(qkeys.items[i].data, qkeys.items[i].len);
            if (!bloom_json_has(m.f_params, key)) {
                bloom_str raw = bloom_str_from_bytes(
                    qvalues.items[i].data,
                    qvalues.items[i].len
                );
                bloom_struct_std_json_JsonValue value = bloom_json_make_string(raw);
                bloom_str_drop(&raw);
                bloom_enum_Result_bool_str set = bloom_json_set(m.f_params, key, value);
                if (set.tag == 1) {
                    bloom_str_drop(&set.data.m_Err._0);
                }
                bloom_json_release(&value.f_id);
            }
            bloom_str_drop(&key);
        }
    } else {
        /* 405 if the path matches other methods, 404 otherwise. */
        char allow[512];
        size_t allow_len = 0;
        allow[0] = '\0';
        bool any = false;
        for (size_t r = 0; r < rt->count; r++) {
            bloom_web_route_def *route = &rt->routes[r];
            if (route->method_len == method_len
                && memcmp(route->method, method_buf, method_len) == 0) {
                continue;
            }
            if (!bloom_web_route_matches(route, &segs)) {
                continue;
            }
            any = true;
            /* Deduplicated, sorted insertion. */
            bool present = false;
            size_t at = 0;
            size_t insert_at = 0;
            while (at < allow_len) {
                size_t end = at;
                while (end < allow_len && allow[end] != ',') {
                    end++;
                }
                size_t mlen = end - at;
                if (mlen == route->method_len
                    && memcmp(allow + at, route->method, route->method_len) == 0) {
                    present = true;
                    break;
                }
                if (memcmp(route->method, allow + at,
                           mlen < route->method_len ? mlen : route->method_len) > 0) {
                    insert_at = end + 2;
                }
                at = end + 2;
            }
            if (present) {
                continue;
            }
            if (allow_len + route->method_len + 2 < sizeof allow) {
                if (insert_at > allow_len) {
                    insert_at = allow_len;
                }
                memmove(
                    allow + insert_at + route->method_len + 2,
                    allow + insert_at,
                    allow_len - insert_at + 1
                );
                memcpy(allow + insert_at, route->method, route->method_len);
                allow[insert_at + route->method_len] = ',';
                allow[insert_at + route->method_len + 1] = ' ';
                allow_len += route->method_len + 2;
            }
        }
        if (any && allow_len >= 2) {
            allow_len -= 2; /* drop the trailing ", " */
            allow[allow_len] = '\0';
            m.f_status = 405;
            bloom_str_drop(&m.f_allow);
            m.f_allow = bloom_str_from_bytes(allow, allow_len);
        } else {
            m.f_status = 404;
        }
    }
    bloom_web_list_free(&segs);
    bloom_web_list_free(&qkeys);
    bloom_web_list_free(&qvalues);
    result.tag = 0;
    result.data.m_Ok._0 = m;
    return result;
}

bloom_enum_Result_std_web_Match_str bloom_web_resolve_request(
    bloom_struct_std_web_Router router,
    bloom_struct_std_http_HttpRequest request
) {
    return bloom_web_resolve(router, request.f_method, request.f_target);
}

/* ---- JSON helpers ------------------------------------------------------- */

bloom_enum_Result_std_json_JsonValue_str bloom_web_json_body(bloom_str body) {
    if (body.len == 0) {
        return bloom_json_err_value(bloom_web_msg(
            "json_body",
            "empty request body"
        ));
    }
    return bloom_json_parse(body);
}

bloom_enum_Result_std_json_JsonValue_str bloom_web_json_body_request(
    bloom_struct_std_http_HttpRequest request
) {
    return bloom_web_json_body(request.f_body);
}

bloom_enum_Result_bool_str bloom_web_json_response(
    int64_t connection,
    int64_t status,
    bloom_struct_std_json_JsonValue value
) {
    bloom_str empty;
    empty.rc = 0;
    empty.data = NULL;
    empty.len = 0;
    bloom_str json_type = bloom_str_from_bytes("application/json", 16);
    bloom_enum_Result_str_str text = bloom_json_stringify(value);
    if (text.tag == 1) {
        bloom_str_drop(&json_type);
        return bloom_web_err_bool(text.data.m_Err._0);
    }
    bloom_enum_Result_bool_str written = bloom_http_write_response_full(
        connection,
        status,
        json_type,
        empty,
        text.data.m_Ok._0
    );
    bloom_str_drop(&json_type);
    bloom_str_drop(&text.data.m_Ok._0);
    return written;
}


bloom_enum_Result_bool_str bloom_web_json_response_with(
    int64_t connection,
    int64_t status,
    bloom_struct_std_json_JsonValue value,
    bloom_str headers
) {
    bloom_str json_type = bloom_str_from_bytes("application/json", 16);
    bloom_enum_Result_str_str text = bloom_json_stringify(value);
    if (text.tag == 1) {
        bloom_str_drop(&json_type);
        return bloom_web_err_bool(text.data.m_Err._0);
    }
    bloom_enum_Result_bool_str written = bloom_http_write_response_full(
        connection,
        status,
        json_type,
        headers,
        text.data.m_Ok._0
    );
    bloom_str_drop(&json_type);
    bloom_str_drop(&text.data.m_Ok._0);
    return written;
}

/* ---- static files -------------------------------------------------------- */

/* Removes a trailing separator-free copy: returns the extension (after the
 * last dot of the file name) or an empty slice. */
static void bloom_web_extension(const char *name, size_t len, const char **ext, size_t *ext_len) {
    size_t dot = len;
    for (size_t i = 0; i < len; i++) {
        if (name[i] == '.') {
            dot = i;
        }
    }
    if (dot >= len || dot == 0) {
        *ext = "";
        *ext_len = 0;
        return;
    }
    *ext = name + dot + 1;
    *ext_len = len - dot - 1;
}

static bool bloom_web_ext_eq(const char *ext, size_t ext_len, const char *want) {
    size_t n = strlen(want);
    if (ext_len != n) {
        return false;
    }
    for (size_t i = 0; i < n; i++) {
        char c = ext[i];
        if (c >= 'A' && c <= 'Z') {
            c = (char)(c + 32);
        }
        if (c != want[i]) {
            return false;
        }
    }
    return true;
}

/* MIME type by file extension (documented, fixed table). Unknown
 * extensions use `application/octet-stream`. */
static const char *bloom_web_mime(const char *name, size_t len) {
    const char *ext = "";
    size_t ext_len = 0;
    bloom_web_extension(name, len, &ext, &ext_len);
    if (bloom_web_ext_eq(ext, ext_len, "html") || bloom_web_ext_eq(ext, ext_len, "htm")) {
        return "text/html; charset=utf-8";
    }
    if (bloom_web_ext_eq(ext, ext_len, "css")) {
        return "text/css; charset=utf-8";
    }
    if (bloom_web_ext_eq(ext, ext_len, "js")) {
        return "text/javascript; charset=utf-8";
    }
    if (bloom_web_ext_eq(ext, ext_len, "json")) {
        return "application/json";
    }
    if (bloom_web_ext_eq(ext, ext_len, "svg")) {
        return "image/svg+xml";
    }
    if (bloom_web_ext_eq(ext, ext_len, "txt")) {
        return "text/plain; charset=utf-8";
    }
    if (bloom_web_ext_eq(ext, ext_len, "png")) {
        return "image/png";
    }
    if (bloom_web_ext_eq(ext, ext_len, "jpg") || bloom_web_ext_eq(ext, ext_len, "jpeg")) {
        return "image/jpeg";
    }
    if (bloom_web_ext_eq(ext, ext_len, "gif")) {
        return "image/gif";
    }
    if (bloom_web_ext_eq(ext, ext_len, "webp")) {
        return "image/webp";
    }
    if (bloom_web_ext_eq(ext, ext_len, "ico")) {
        return "image/x-icon";
    }
    if (bloom_web_ext_eq(ext, ext_len, "woff")) {
        return "font/woff";
    }
    if (bloom_web_ext_eq(ext, ext_len, "woff2")) {
        return "font/woff2";
    }
    return "application/octet-stream";
}

/* Case-insensitive comparison of the request method with `method`. */
static bool bloom_web_req_method_is(
    bloom_struct_std_http_HttpRequest *request,
    const char *method
) {
    size_t n = strlen(method);
    if (request->f_method.len != n) {
        return false;
    }
    for (size_t i = 0; i < n; i++) {
        char c = request->f_method.data[i];
        if (c >= 'a' && c <= 'z') {
            c = (char)(c + 32);
        }
        char m = method[i];
        if (m >= 'a' && m <= 'z') {
            m = (char)(m + 32);
        }
        if (c != m) {
            return false;
        }
    }
    return true;
}

/* Sends a plain-text error (without a body for HEAD requests) and returns
 * its status. */
static bloom_enum_Result_i64_str bloom_web_static_error(
    int64_t connection,
    int64_t status,
    bloom_str text,
    bool suppress_body
) {
    bloom_str content_type = bloom_str_from_bytes("text/plain; charset=utf-8", 25);
    bloom_str headers;
    headers.rc = 0;
    headers.data = NULL;
    headers.len = 0;
    if (status == 405) {
        headers = bloom_str_from_bytes("Allow: GET, HEAD", 16);
    }
    bloom_enum_Result_bool_str written = bloom_http_write_response_impl(
        connection,
        status,
        content_type,
        headers,
        text.data,
        text.len,
        suppress_body,
        "static_file"
    );
    bloom_str_drop(&content_type);
    bloom_str_drop(&headers);
    bloom_str_drop(&text);
    bloom_enum_Result_i64_str result;
    if (written.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = written.data.m_Err._0;
        return result;
    }
    result.tag = 0;
    result.data.m_Ok._0 = status;
    return result;
}

/* Serves one static file from `root` for `request`.
 *
 * - Only GET and HEAD are supported (405 with `Allow: GET, HEAD` otherwise).
 * - The URL path is split first and percent-decoded exactly once; invalid
 *   escapes, decoded separators (`/`, `\\`), `..` segments, absolute paths
 *   and drive/UNC prefixes are rejected with 400.
 * - The opened file (following symbolic links/junctions at open time) must
 *   resolve to a location inside `root`; otherwise 400. `std::path`
 *   normalization alone is not used as a security boundary.
 * - Missing files/directories: 404. Other open/read failures and files
 *   above the 8 MiB limit: 500.
 * - Content-Type comes from the extension table; `X-Content-Type-Options:
 *   nosniff` is always sent. HEAD sends the same headers (real
 *   Content-Length) without a body.
 * - Responses are fully buffered (no streaming, no range requests).
 *
 * Returns `Ok(status)` with the status that was sent, or `Err` when the
 * response itself could not be written.
 */
bloom_enum_Result_i64_str bloom_web_static_file(
    int64_t connection,
    bloom_str root,
    bloom_struct_std_http_HttpRequest request
) {
    bool is_get = bloom_web_req_method_is(&request, "GET");
    bool is_head = bloom_web_req_method_is(&request, "HEAD");
    if (!is_get && !is_head) {
        bloom_str msg = bloom_str_from_bytes("method not allowed", 18);
        return bloom_web_static_error(connection, 405, msg, is_head);
    }
    size_t qmark = request.f_target.len;
    for (size_t i = 0; i < request.f_target.len; i++) {
        if (request.f_target.data[i] == '?') {
            qmark = i;
            break;
        }
    }
    bloom_web_list segs;
    bloom_web_list_init(&segs);
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    bool split_ok = bloom_web_split_path(request.f_target.data, qmark, &segs, &err) == 0;
    size_t seg_count = segs.count;
    if (!split_ok || seg_count == 0) {
        bloom_str_drop(&err);
        bloom_web_list_free(&segs);
        if (split_ok) {
            bloom_str msg = bloom_str_from_bytes("not found", 9);
            return bloom_web_static_error(connection, 404, msg, is_head);
        }
        bloom_str msg = bloom_str_from_bytes("bad request", 11);
        return bloom_web_static_error(connection, 400, msg, is_head);
    }
    /* Build the relative filesystem path from the decoded segments,
     * rejecting decoded separators (they would smuggle path structure). */
    bloom_http_buf rel;
    bloom_http_buf_init(&rel);
    for (size_t i = 0; i < segs.count; i++) {
        for (size_t k = 0; k < segs.items[i].len; k++) {
            char c = segs.items[i].data[k];
            if (c == '/' || c == '\\') {
                bloom_http_buf_free(&rel);
                bloom_web_list_free(&segs);
                bloom_str msg = bloom_str_from_bytes("bad request", 11);
                return bloom_web_static_error(connection, 400, msg, is_head);
            }
        }
        if (i > 0) {
            bloom_http_buf_append(&rel, "/", 1);
        }
        bloom_http_buf_append(&rel, segs.items[i].data, segs.items[i].len);
    }
    bloom_web_list_free(&segs);
    bloom_str rel_path = bloom_str_from_bytes(rel.data != NULL ? rel.data : "", rel.len);
    bloom_http_buf_free(&rel);
    char *joined = NULL;
    size_t joined_len = 0;
    bloom_str join_err;
    join_err.rc = 0;
    join_err.data = NULL;
    join_err.len = 0;
    if (!bloom_fs_join_inside_root(root, rel_path, &joined, &joined_len, &join_err)) {
        bloom_str_drop(&rel_path);
        bloom_str_drop(&join_err);
        bloom_str msg = bloom_str_from_bytes("bad request", 11);
        return bloom_web_static_error(connection, 400, msg, is_head);
    }
    bloom_str joined_str = bloom_str_from_bytes(joined, joined_len);
    free(joined);
    FILE *file = NULL;
    size_t size = 0;
    bloom_str open_err;
    open_err.rc = 0;
    open_err.data = NULL;
    open_err.len = 0;
    bloom_fs_open_status st = bloom_fs_open_confined(
        root,
        joined_str,
        BLOOM_FS_MAX_BYTES,
        &file,
        &size,
        &open_err
    );
    if (st != BLOOM_FS_OPEN_OK) {
        bloom_str_drop(&joined_str);
        bloom_str_drop(&rel_path);
        bloom_str_drop(&open_err);
        bloom_str msg;
        if (st == BLOOM_FS_OPEN_NOT_FOUND) {
            msg = bloom_str_from_bytes("not found", 9);
            return bloom_web_static_error(connection, 404, msg, is_head);
        }
        if (st == BLOOM_FS_OPEN_OUTSIDE) {
            msg = bloom_str_from_bytes("bad request", 11);
            return bloom_web_static_error(connection, 400, msg, is_head);
        }
        msg = bloom_str_from_bytes("internal error", 14);
        return bloom_web_static_error(connection, 500, msg, is_head);
    }
    unsigned char *data = NULL;
    size_t data_len = 0;
    bool read_ok = bloom_fs_read_all(file, size, &data, &data_len);
    fclose(file);
    if (!read_ok) {
        bloom_str_drop(&joined_str);
        bloom_str_drop(&rel_path);
        bloom_str msg = bloom_str_from_bytes("internal error", 14);
        return bloom_web_static_error(connection, 500, msg, is_head);
    }
    bloom_struct_std_fs_Bytes bytes = bloom_fs_bytes_take(data, data_len);
    bloom_struct_std_fs_Bytes content = bytes;
    const char *mime = bloom_web_mime(
        rel_path.data != NULL ? rel_path.data : "",
        rel_path.len
    );
    bloom_str mime_str = bloom_str_from_bytes(mime, strlen(mime));
    bloom_str headers = bloom_str_from_bytes(
        "X-Content-Type-Options: nosniff",
        31
    );
    bloom_enum_Result_bool_str written = is_head
        ? bloom_http_write_response_bytes_head(
              connection,
              200,
              mime_str,
              headers,
              content
          )
        : bloom_http_write_response_bytes(
              connection,
              200,
              mime_str,
              headers,
              content
          );
    bloom_str_drop(&mime_str);
    bloom_str_drop(&headers);
    bloom_str_drop(&rel_path);
    bloom_str_drop(&joined_str);
    bloom_fs_bytes_release(&bytes.f_id);
    bloom_enum_Result_i64_str result;
    if (written.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = written.data.m_Err._0;
        return result;
    }
    result.tag = 0;
    result.data.m_Ok._0 = 200;
    return result;
}

/* ---- forms (application/x-www-form-urlencoded) -------------------------- */

/*
 * Form data keeps every name/value pair in the order it appeared in the
 * request. Repeated names (checkbox groups, multi-selects) are therefore
 * preserved instead of being overwritten. Names and values are ordinary
 * reference-counted strings; the form owns one reference to each of them.
 *
 * Limits (documented in docs/LANGUAGE_SPEC.md):
 *   body <= 1 MiB, <= 4096 entries, name <= 256 bytes, value <= 64 KiB.
 */
#define BLOOM_FORM_MAX_BYTES (1024 * 1024)
#define BLOOM_FORM_MAX_ENTRIES 4096
#define BLOOM_FORM_MAX_NAME 256
#define BLOOM_FORM_MAX_VALUE (64 * 1024)

typedef struct {
    bloom_str name;
    bloom_str value;
} bloom_form_entry;

typedef struct {
    size_t rc;
    bloom_form_entry *entries;
    size_t count;
    size_t cap;
} bloom_web_form;

static bloom_str bloom_web_lit(const char *p, size_t n) {
    bloom_str s;
    s.rc = BLOOM_STR_IMMORTAL;
    s.data = (char *)p;
    s.len = n;
    return s;
}

static bloom_struct_std_web_FormData bloom_web_form_wrap(bloom_web_form *f) {
    bloom_struct_std_web_FormData v;
    v.f_id = (bloom_form_handle)(intptr_t)f;
    return v;
}

static bloom_web_form *bloom_web_form_unwrap(bloom_struct_std_web_FormData v) {
    return (bloom_web_form *)(intptr_t)v.f_id;
}

bloom_form_handle bloom_web_form_retain(bloom_form_handle h) {
    bloom_web_form *f = (bloom_web_form *)(intptr_t)h;
    if (f != NULL) {
        f->rc++;
    }
    return h;
}

void bloom_web_form_release(bloom_form_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_web_form *f = (bloom_web_form *)(intptr_t)*h;
    *h = 0;
    if (f->rc > 0) {
        f->rc--;
    }
    if (f->rc == 0) {
        for (size_t i = 0; i < f->count; i++) {
            bloom_str_drop(&f->entries[i].name);
            bloom_str_drop(&f->entries[i].value);
        }
        free(f->entries);
        free(f);
        bloom_live_allocs--;
    }
}

static bloom_web_form *bloom_web_form_new(void) {
    bloom_web_form *f = (bloom_web_form *)calloc(1, sizeof(bloom_web_form));
    if (f == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    f->rc = 1;
    bloom_live_allocs++;
    return f;
}

/* Takes ownership of name and value. */
static void bloom_web_form_push(
    bloom_web_form *f,
    bloom_str name,
    bloom_str value
) {
    if (f->count >= BLOOM_FORM_MAX_ENTRIES) {
        bloom_str_drop(&name);
        bloom_str_drop(&value);
        bloom_panicf(NULL, 0, "too many form entries");
    }
    if (f->count == f->cap) {
        size_t cap = f->cap ? f->cap * 2 : 8;
        bloom_form_entry *ne = (bloom_form_entry *)realloc(
            f->entries,
            cap * sizeof *ne
        );
        if (ne == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        f->entries = ne;
        f->cap = cap;
    }
    f->entries[f->count].name = name;
    f->entries[f->count].value = value;
    f->count++;
}

static bloom_enum_Result_std_web_FormData_str bloom_web_form_ok_form(
    bloom_struct_std_web_FormData v
) {
    bloom_enum_Result_std_web_FormData_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_std_web_FormData_str bloom_web_form_err(
    bloom_str m
) {
    bloom_enum_Result_std_web_FormData_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Option_str bloom_web_form_some(bloom_str v) {
    bloom_enum_Option_str r;
    r.tag = 0;
    r.data.m_Some._0 = v;
    return r;
}

static bloom_enum_Option_str bloom_web_form_none(void) {
    bloom_enum_Option_str r;
    r.tag = 1;
    return r;
}

static bloom_enum_Result_str_str bloom_web_form_ok_str(bloom_str v) {
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_str_str bloom_web_form_err_str(bloom_str m) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bool bloom_web_form_name_eq(bloom_str a, bloom_str b) {
    if (a.len != b.len) {
        return false;
    }
    if (a.len == 0) {
        return true;
    }
    return memcmp(a.data, b.data, a.len) == 0;
}

/*
 * Parses `a=1&b=2` with form-style decoding. `&` separates entries, the
 * first `=` separates name and value, `+` is a space, percent escapes are
 * decoded exactly once and the result must be valid UTF-8. Empty segments
 * are skipped; an entry without `=` has an empty value. Empty names and
 * empty values are kept because they are different from a missing field.
 */
static void bloom_web_form_abort(bloom_web_form *f) {
    for (size_t i = 0; i < f->count; i++) {
        bloom_str_drop(&f->entries[i].name);
        bloom_str_drop(&f->entries[i].value);
    }
    free(f->entries);
    free(f);
    bloom_live_allocs--;
}

static bloom_enum_Result_std_web_FormData_str bloom_web_form_parse(
    bloom_str body,
    const char *op
) {
    if (body.len > BLOOM_FORM_MAX_BYTES) {
        return bloom_web_form_err(bloom_web_msg(
            op,
            "form body exceeds 1048576 bytes"
        ));
    }
    bloom_web_form *f = bloom_web_form_new();
    size_t i = 0;
    while (i < body.len) {
        size_t start = i;
        while (i < body.len && body.data[i] != '&') {
            i++;
        }
        size_t part_len = i - start;
        if (i < body.len) {
            i++;
        }
        if (part_len == 0) {
            continue;
        }
        size_t eq = 0;
        while (eq < part_len && body.data[start + eq] != '=') {
            eq++;
        }
        size_t name_raw = eq;
        size_t value_raw = eq < part_len ? part_len - eq - 1 : 0;
        const char *value_start = body.data + start
            + (eq < part_len ? eq + 1 : part_len);
        char *name = NULL;
        size_t name_len = 0;
        int rc = bloom_web_decode(
            body.data + start,
            name_raw,
            true,
            &name,
            &name_len
        );
        if (rc != 0) {
            bloom_web_form_abort(f);
            return bloom_web_form_err(bloom_web_msg(
                op,
                rc == 1
                    ? "invalid percent-encoding in field name"
                    : "invalid UTF-8 in decoded field name"
            ));
        }
        if (name_len > BLOOM_FORM_MAX_NAME) {
            free(name);
            bloom_web_form_abort(f);
            return bloom_web_form_err(bloom_web_msg(
                op,
                "field name exceeds 256 bytes"
            ));
        }
        char *value = NULL;
        size_t value_len = 0;
        rc = bloom_web_decode(value_start, value_raw, true, &value, &value_len);
        if (rc != 0) {
            free(name);
            bloom_web_form_abort(f);
            return bloom_web_form_err(bloom_web_msg(
                op,
                rc == 1
                    ? "invalid percent-encoding in field value"
                    : "invalid UTF-8 in decoded field value"
            ));
        }
        if (value_len > BLOOM_FORM_MAX_VALUE) {
            free(name);
            free(value);
            bloom_web_form_abort(f);
            return bloom_web_form_err(bloom_web_msg(
                op,
                "field value exceeds 65536 bytes"
            ));
        }
        if (f->count >= BLOOM_FORM_MAX_ENTRIES) {
            free(name);
            free(value);
            bloom_web_form_abort(f);
            return bloom_web_form_err(bloom_web_msg(
                op,
                "too many form entries (max 4096)"
            ));
        }
        bloom_str entry_name = bloom_str_from_bytes(name, name_len);
        bloom_str entry_value = bloom_str_from_bytes(value, value_len);
        free(name);
        free(value);
        bloom_web_form_push(f, entry_name, entry_value);
    }
    return bloom_web_form_ok_form(bloom_web_form_wrap(f));
}

/* Parses a URL-encoded body directly (no Content-Type is consulted). */
bloom_enum_Result_std_web_FormData_str bloom_web_form_body(bloom_str body) {
    return bloom_web_form_parse(body, "form_body");
}

static bool bloom_web_ct_ci_eq(const char *a, size_t n, const char *b) {
    size_t bn = strlen(b);
    if (n != bn) {
        return false;
    }
    for (size_t i = 0; i < n; i++) {
        char c = a[i];
        char d = b[i];
        if (c >= 'A' && c <= 'Z') {
            c = (char)(c + 32);
        }
        if (d >= 'A' && d <= 'Z') {
            d = (char)(d + 32);
        }
        if (c != d) {
            return false;
        }
    }
    return true;
}

/*
 * Checks the media type and looks for a charset parameter. Any unknown
 * parameter is ignored; a charset other than UTF-8 is rejected because
 * decoded form values must be UTF-8 strings.
 */
static bool bloom_web_ct_is_form(bloom_str ct, bool *bad_charset) {
    *bad_charset = false;
    size_t end = 0;
    while (end < ct.len && ct.data[end] != ';') {
        end++;
    }
    size_t start = 0;
    while (start < end && (ct.data[start] == ' ' || ct.data[start] == '\t')) {
        start++;
    }
    while (end > start && (ct.data[end - 1] == ' ' || ct.data[end - 1] == '\t')) {
        end--;
    }
    if (!bloom_web_ct_ci_eq(
            ct.data + start,
            end - start,
            "application/x-www-form-urlencoded"
        )) {
        return false;
    }
    size_t i = end;
    while (i < ct.len) {
        if (ct.data[i] == ';') {
            i++;
        }
        while (i < ct.len && (ct.data[i] == ' ' || ct.data[i] == '\t')) {
            i++;
        }
        size_t pstart = i;
        while (i < ct.len && ct.data[i] != '=' && ct.data[i] != ';') {
            i++;
        }
        size_t pend = i;
        while (pend > pstart
               && (ct.data[pend - 1] == ' ' || ct.data[pend - 1] == '\t')) {
            pend--;
        }
        size_t vstart = i;
        size_t vend = i;
        if (i < ct.len && ct.data[i] == '=') {
            i++;
            vstart = i;
            while (i < ct.len && ct.data[i] != ';') {
                i++;
            }
            vend = i;
        }
        while (vstart < vend
               && (ct.data[vstart] == ' ' || ct.data[vstart] == '\t')) {
            vstart++;
        }
        while (vend > vstart
               && (ct.data[vend - 1] == ' ' || ct.data[vend - 1] == '\t')) {
            vend--;
        }
        if (vend >= vstart + 2 && ct.data[vstart] == '"'
            && ct.data[vend - 1] == '"') {
            vstart++;
            vend--;
        }
        if (bloom_web_ct_ci_eq(ct.data + pstart, pend - pstart, "charset")) {
            if (!bloom_web_ct_ci_eq(ct.data + vstart, vend - vstart, "utf-8")
                && !bloom_web_ct_ci_eq(ct.data + vstart, vend - vstart, "utf8")) {
                *bad_charset = true;
            }
        }
    }
    return true;
}

/*
 * Parses a request body as a form. The Content-Type header must be
 * `application/x-www-form-urlencoded` (parameters are allowed, an explicit
 * charset must be UTF-8). An empty body is a valid, empty form.
 */
bloom_enum_Result_std_web_FormData_str bloom_web_form_request(
    bloom_struct_std_http_HttpRequest request
) {
    bloom_enum_Option_str ct = bloom_http_header_get(
        request.f_headers,
        bloom_web_lit("content-type", 12)
    );
    if (ct.tag != 0) {
        return bloom_web_form_err(bloom_web_msg(
            "form_body_request",
            "missing Content-Type header"
        ));
    }
    bloom_str value = ct.data.m_Some._0;
    bool bad_charset = false;
    bool is_form = bloom_web_ct_is_form(value, &bad_charset);
    bloom_str_drop(&value);
    if (!is_form) {
        return bloom_web_form_err(bloom_web_msg(
            "form_body_request",
            "unsupported Content-Type (expected application/x-www-form-urlencoded)"
        ));
    }
    if (bad_charset) {
        return bloom_web_form_err(bloom_web_msg(
            "form_body_request",
            "unsupported charset (only UTF-8)"
        ));
    }
    return bloom_web_form_parse(request.f_body, "form_body_request");
}

int64_t bloom_web_form_count(bloom_struct_std_web_FormData form) {
    bloom_web_form *f = bloom_web_form_unwrap(form);
    if (f == NULL) {
        return 0;
    }
    return (int64_t)f->count;
}

bool bloom_web_form_has(bloom_struct_std_web_FormData form, bloom_str name) {
    bloom_web_form *f = bloom_web_form_unwrap(form);
    if (f == NULL) {
        return false;
    }
    for (size_t i = 0; i < f->count; i++) {
        if (bloom_web_form_name_eq(f->entries[i].name, name)) {
            return true;
        }
    }
    return false;
}

/*
 * Returns the value of a field that occurs exactly once. A missing field
 * and a field with several values are errors, so a program cannot
 * accidentally treat the first of many values as "the" value. An existing
 * field with an empty value returns Ok("").
 */
bloom_enum_Result_str_str bloom_web_form_get(
    bloom_struct_std_web_FormData form,
    bloom_str name
) {
    bloom_web_form *f = bloom_web_form_unwrap(form);
    if (f == NULL) {
        return bloom_web_form_err_str(bloom_web_msg(
            "form_get",
            "invalid form data"
        ));
    }
    bloom_str found = {0, NULL, 0};
    size_t count = 0;
    for (size_t i = 0; i < f->count; i++) {
        if (bloom_web_form_name_eq(f->entries[i].name, name)) {
            count++;
            if (count == 1) {
                found = f->entries[i].value;
            }
        }
    }
    if (count == 0) {
        return bloom_web_form_err_str(bloom_web_msg(
            "form_get",
            "field not found"
        ));
    }
    if (count > 1) {
        return bloom_web_form_err_str(bloom_web_msg(
            "form_get",
            "field has multiple values"
        ));
    }
    return bloom_web_form_ok_str(bloom_str_clone(found));
}

bloom_enum_Option_str bloom_web_form_first(
    bloom_struct_std_web_FormData form,
    bloom_str name
) {
    bloom_web_form *f = bloom_web_form_unwrap(form);
    if (f == NULL) {
        return bloom_web_form_none();
    }
    for (size_t i = 0; i < f->count; i++) {
        if (bloom_web_form_name_eq(f->entries[i].name, name)) {
            return bloom_web_form_some(bloom_str_clone(f->entries[i].value));
        }
    }
    return bloom_web_form_none();
}

bloom_arr_str bloom_web_form_values(
    bloom_struct_std_web_FormData form,
    bloom_str name
) {
    bloom_arr_str out = bloom_arr_str_new();
    bloom_web_form *f = bloom_web_form_unwrap(form);
    if (f != NULL) {
        for (size_t i = 0; i < f->count; i++) {
            if (bloom_web_form_name_eq(f->entries[i].name, name)) {
                bloom_arr_str_push_owned(
                    out,
                    bloom_str_clone(f->entries[i].value)
                );
            }
        }
    }
    return out;
}

/* Distinct field names in first-occurrence order (for generic iteration). */
bloom_arr_str bloom_web_form_names(bloom_struct_std_web_FormData form) {
    bloom_arr_str out = bloom_arr_str_new();
    bloom_web_form *f = bloom_web_form_unwrap(form);
    if (f == NULL) {
        return out;
    }
    for (size_t i = 0; i < f->count; i++) {
        bool seen = false;
        for (size_t k = 0; k < i && !seen; k++) {
            if (bloom_web_form_name_eq(f->entries[k].name, f->entries[i].name)) {
                seen = true;
            }
        }
        if (!seen) {
            bloom_arr_str_push_owned(out, bloom_str_clone(f->entries[i].name));
        }
    }
    return out;
}

static bool bloom_web_form_unreserved(unsigned char c) {
    if (c >= 'a' && c <= 'z') {
        return true;
    }
    if (c >= 'A' && c <= 'Z') {
        return true;
    }
    if (c >= '0' && c <= '9') {
        return true;
    }
    return c == '-' || c == '.' || c == '_' || c == '~';
}

static size_t bloom_web_form_encoded_len(bloom_str s) {
    size_t n = 0;
    for (size_t i = 0; i < s.len; i++) {
        unsigned char c = (unsigned char)s.data[i];
        n += bloom_web_form_unreserved(c) ? 1 : (c == ' ' ? 1 : 3);
    }
    return n;
}

static void bloom_web_form_encode_into(char *out, size_t *at, bloom_str s) {
    static const char hex[] = "0123456789ABCDEF";
    for (size_t i = 0; i < s.len; i++) {
        unsigned char c = (unsigned char)s.data[i];
        if (bloom_web_form_unreserved(c)) {
            out[(*at)++] = (char)c;
        } else if (c == ' ') {
            out[(*at)++] = '+';
        } else {
            out[(*at)++] = '%';
            out[(*at)++] = hex[c >> 4];
            out[(*at)++] = hex[c & 15];
        }
    }
}

/*
 * Encodes the form in entry order. Unreserved bytes stay as-is, spaces
 * become `+`, everything else becomes `%XX` (uppercase). The result must
 * fit into the same 1 MiB limit that the parser accepts, so whatever this
 * function produces can be parsed again with identical entries and order.
 */
bloom_enum_Result_str_str bloom_web_form_encode(
    bloom_struct_std_web_FormData form
) {
    bloom_web_form *f = bloom_web_form_unwrap(form);
    if (f == NULL) {
        return bloom_web_form_err_str(bloom_web_msg(
            "form_encode",
            "invalid form data"
        ));
    }
    size_t total = 0;
    for (size_t i = 0; i < f->count; i++) {
        size_t add = bloom_web_form_encoded_len(f->entries[i].name) + 1
            + bloom_web_form_encoded_len(f->entries[i].value);
        if (i > 0) {
            add += 1;
        }
        if (add > BLOOM_FORM_MAX_BYTES - total) {
            return bloom_web_form_err_str(bloom_web_msg(
                "form_encode",
                "encoded form exceeds 1048576 bytes"
            ));
        }
        total += add;
    }
    char *buf = (char *)malloc(total + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t at = 0;
    for (size_t i = 0; i < f->count; i++) {
        if (i > 0) {
            buf[at++] = '&';
        }
        bloom_web_form_encode_into(buf, &at, f->entries[i].name);
        buf[at++] = '=';
        bloom_web_form_encode_into(buf, &at, f->entries[i].value);
    }
    bloom_str result = bloom_str_from_bytes(buf, at);
    free(buf);
    return bloom_web_form_ok_str(result);
}
