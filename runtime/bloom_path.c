/* Bloom std::path runtime — lexical path manipulation.
 *
 * Every function in this file works on the *text* of a path; none of them
 * touches the file system. The semantics follow the host platform:
 *
 *   Windows: separators are `\` and `/` (output uses `\`), drive prefixes
 *            (`C:`) and UNC prefixes (`\\server\share`) are recognized.
 *            `\\?\` and `\\.\` device prefixes are rejected with an error.
 *   POSIX:   separator is `/`; `\` is an ordinary file name character.
 *
 * `normalize` is a purely lexical clean-up in the style of Go's
 * `filepath.Clean`: it collapses separators, removes `.` components,
 * resolves `..` against preceding components and keeps leading `..`
 * components of relative paths. It never resolves symbolic links, junctions,
 * 8.3 short names, case or any other real file system property.
 *
 * Error contract: `normalize` returns `Result<string, string>` with a clear
 * message for unsupported forms (empty path, device prefixes, incomplete
 * UNC paths). The other functions never fail; they return `Option<string>`
 * where a component may be absent.
 */

/* ---- Result/Option constructors ---------------------------------------- */

static bloom_enum_Result_str_str bloom_path_ok_str(bloom_str v) {
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_str_str bloom_path_err_str(bloom_str m) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Option_str bloom_path_some(bloom_str v) {
    bloom_enum_Option_str r;
    r.tag = 0;
    r.data.m_Some._0 = v;
    return r;
}

static bloom_enum_Option_str bloom_path_none(void) {
    bloom_enum_Option_str r;
    r.tag = 1;
    return r;
}

static bloom_str bloom_path_err(const char *op, const char *detail) {
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

/* ---- lexical scanner ---------------------------------------------------- */

typedef struct {
    const char *p;
    size_t len;
} bloom_path_span;

typedef struct {
    /* Canonical prefix text (without trailing separator): `C:`, `\\s\share`
     * or empty. */
    char prefix[600];
    size_t prefix_len;
    bool has_prefix;
    bool has_root;
    bloom_path_span *comps;
    size_t comp_count;
    size_t comp_cap;
} bloom_path_split;

static bool bloom_path_is_sep(char c) {
#ifdef _WIN32
    return c == '\\' || c == '/';
#else
    return c == '/';
#endif
}

static char bloom_path_sep(void) {
#ifdef _WIN32
    return '\\';
#else
    return '/';
#endif
}

static bool bloom_path_is_alpha(char c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
}

static void bloom_path_split_free(bloom_path_split *s) {
    free(s->comps);
    s->comps = NULL;
    s->comp_count = 0;
    s->comp_cap = 0;
}

static void bloom_path_push_comp(bloom_path_split *s, const char *p, size_t len) {
    if (s->comp_count == s->comp_cap) {
        size_t cap = s->comp_cap ? s->comp_cap * 2 : 8;
        bloom_path_span *nd = (bloom_path_span *)realloc(s->comps, cap * sizeof *nd);
        if (nd == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        s->comps = nd;
        s->comp_cap = cap;
    }
    s->comps[s->comp_count].p = p;
    s->comps[s->comp_count].len = len;
    s->comp_count++;
}

/* Splits `path` into prefix, root and components. Returns NULL on success or
 * a static error detail string for unsupported forms. */
static const char *bloom_path_split_path(
    const char *p,
    size_t len,
    bloom_path_split *out
) {
    out->prefix_len = 0;
    out->has_prefix = false;
    out->has_root = false;
    out->comps = NULL;
    out->comp_count = 0;
    out->comp_cap = 0;
    size_t i = 0;
#ifdef _WIN32
    /* Extended-length and device prefixes are not lexically safe to rewrite. */
    if (len >= 4 && bloom_path_is_sep(p[0]) && bloom_path_is_sep(p[1])
        && (p[2] == '?' || p[2] == '.') && bloom_path_is_sep(p[3])) {
        return "extended-length (`\\\\?\\`) and device (`\\\\.\\`) prefixes are not supported";
    }
    if (len >= 2 && bloom_path_is_sep(p[0]) && bloom_path_is_sep(p[1])) {
        /* UNC: \\server\share */
        size_t j = 2;
        size_t server_start = j;
        while (j < len && !bloom_path_is_sep(p[j])) {
            j++;
        }
        if (j == server_start) {
            return "UNC path without a server name is not supported";
        }
        size_t server_len = j - server_start;
        while (j < len && bloom_path_is_sep(p[j])) {
            j++;
        }
        size_t share_start = j;
        while (j < len && !bloom_path_is_sep(p[j])) {
            j++;
        }
        if (j == share_start) {
            return "UNC path without a share name is not supported";
        }
        size_t share_len = j - share_start;
        if (2 + server_len + 1 + share_len >= sizeof out->prefix) {
            return "path prefix is too long";
        }
        out->prefix[0] = '\\';
        out->prefix[1] = '\\';
        memcpy(out->prefix + 2, p + server_start, server_len);
        out->prefix[2 + server_len] = '\\';
        memcpy(out->prefix + 2 + server_len + 1, p + share_start, share_len);
        out->prefix_len = 2 + server_len + 1 + share_len;
        out->has_prefix = true;
        out->has_root = true;
        i = j;
    } else if (len >= 2 && bloom_path_is_alpha(p[0]) && p[1] == ':') {
        out->prefix[0] = p[0];
        out->prefix[1] = ':';
        out->prefix_len = 2;
        out->has_prefix = true;
        i = 2;
        if (i < len && bloom_path_is_sep(p[i])) {
            out->has_root = true;
        }
    } else if (len >= 1 && bloom_path_is_sep(p[0])) {
        out->has_root = true;
    }
#else
    if (len >= 1 && p[0] == '/') {
        out->has_root = true;
    }
#endif
    while (i < len) {
        while (i < len && bloom_path_is_sep(p[i])) {
            i++;
        }
        size_t start = i;
        while (i < len && !bloom_path_is_sep(p[i])) {
            i++;
        }
        if (i > start) {
            bloom_path_push_comp(out, p + start, i - start);
        }
    }
    return NULL;
}

/* Rebuilds `prefix`/`root`/components into a fresh string. */
static bloom_str bloom_path_rebuild(
    const bloom_path_split *s,
    const bloom_path_span *comps,
    size_t count
) {
    size_t total = s->prefix_len + (s->has_root ? 1 : 0);
    for (size_t i = 0; i < count; i++) {
        total += comps[i].len + (i > 0 ? 1 : 0);
    }
    bool empty_relative = !s->has_prefix && !s->has_root && count == 0;
    if (empty_relative) {
        total += 1; /* "." */
    }
    char *buf = (char *)malloc(total + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t at = 0;
    memcpy(buf + at, s->prefix, s->prefix_len);
    at += s->prefix_len;
    if (s->has_root) {
        buf[at++] = bloom_path_sep();
    }
    for (size_t i = 0; i < count; i++) {
        if (i > 0) {
            buf[at++] = bloom_path_sep();
        }
        memcpy(buf + at, comps[i].p, comps[i].len);
        at += comps[i].len;
    }
    if (empty_relative) {
        buf[at++] = '.';
    }
    bloom_str out = bloom_str_from_bytes(buf, at);
    free(buf);
    return out;
}

/* ---- component helpers -------------------------------------------------- */

static bool bloom_path_comp_is(const bloom_path_span *c, const char *text) {
    size_t n = strlen(text);
    return c->len == n && memcmp(c->p, text, n) == 0;
}

static bool bloom_path_split_ok(const bloom_path_split *s) {
    return s->comps != NULL || s->comp_count == 0;
}

/* ---- public API --------------------------------------------------------- */

bloom_str bloom_path_join(bloom_str base, bloom_str child) {
    if (child.len == 0) {
        return bloom_str_from_bytes(base.data, base.len);
    }
    bloom_path_split cs;
    const char *err = bloom_path_split_path(child.data, child.len, &cs);
    if (err != NULL) {
        /* Unsupported child forms are returned literally (no rewriting). */
        bloom_path_split_free(&cs);
        return bloom_str_from_bytes(child.data, child.len);
    }
    bool child_absolute = cs.has_prefix || cs.has_root;
    if (cs.has_root && !cs.has_prefix) {
        /* Rooted child (`\foo`): keeps the base prefix. */
        bloom_path_split bs;
        const char *berr = bloom_path_split_path(base.data, base.len, &bs);
        bloom_str out;
        if (berr == NULL && bs.has_prefix) {
            size_t total = bs.prefix_len + 1 + child.len - 1;
            char *buf = (char *)malloc(total + 1);
            if (buf == NULL) {
                bloom_panicf(NULL, 0, "out of memory");
            }
            memcpy(buf, bs.prefix, bs.prefix_len);
            buf[bs.prefix_len] = bloom_path_sep();
            memcpy(buf + bs.prefix_len + 1, child.data + 1, child.len - 1);
            out = bloom_str_from_bytes(buf, total);
            free(buf);
        } else {
            out = bloom_str_from_bytes(child.data, child.len);
        }
        bloom_path_split_free(&bs);
        bloom_path_split_free(&cs);
        return out;
    }
    if (child_absolute) {
        bloom_path_split_free(&cs);
        return bloom_str_from_bytes(child.data, child.len);
    }
    bloom_path_split_free(&cs);
    if (base.len == 0) {
        return bloom_str_from_bytes(child.data, child.len);
    }
    bool base_has_sep = bloom_path_is_sep(base.data[base.len - 1]);
    size_t total = base.len + (base_has_sep ? 0 : 1) + child.len;
    char *buf = (char *)malloc(total + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(buf, base.data, base.len);
    size_t at = base.len;
    if (!base_has_sep) {
        buf[at++] = bloom_path_sep();
    }
    memcpy(buf + at, child.data, child.len);
    at += child.len;
    bloom_str out = bloom_str_from_bytes(buf, at);
    free(buf);
    return out;
}

bloom_enum_Option_str bloom_path_parent(bloom_str path) {
    bloom_path_split s;
    const char *err = bloom_path_split_path(path.data, path.len, &s);
    if (err != NULL) {
        return bloom_path_none();
    }
    if (s.comp_count == 0) {
        bloom_path_split_free(&s);
        return bloom_path_none();
    }
    if (s.comp_count == 1 && !s.has_prefix && !s.has_root) {
        /* The parent of a bare name is the empty path. */
        bloom_path_split_free(&s);
        return bloom_path_some(bloom_str_from_bytes("", 0));
    }
    bloom_str out = bloom_path_rebuild(&s, s.comps, s.comp_count - 1);
    bloom_path_split_free(&s);
    return bloom_path_some(out);
}

bloom_enum_Option_str bloom_path_file_name(bloom_str path) {
    bloom_path_split s;
    const char *err = bloom_path_split_path(path.data, path.len, &s);
    if (err != NULL) {
        return bloom_path_none();
    }
    /* A trailing `..` component is not a file name (Rust-style). */
    while (s.comp_count > 0
           && (bloom_path_comp_is(&s.comps[s.comp_count - 1], ".")
               || bloom_path_comp_is(&s.comps[s.comp_count - 1], ".."))) {
        if (bloom_path_comp_is(&s.comps[s.comp_count - 1], "..")) {
            bloom_path_split_free(&s);
            return bloom_path_none();
        }
        s.comp_count--;
    }
    if (s.comp_count == 0) {
        bloom_path_split_free(&s);
        return bloom_path_none();
    }
    bloom_path_span last = s.comps[s.comp_count - 1];
    bloom_str out = bloom_str_from_bytes(last.p, last.len);
    bloom_path_split_free(&s);
    return bloom_path_some(out);
}

/* Shared by extension/stem: locates the final extension dot. Returns the
 * index of the dot in `name`, or -1 when there is no extension. */
static long bloom_path_ext_dot(const char *name, size_t len) {
    if (len == 0) {
        return -1;
    }
    long dot = -1;
    for (size_t i = 0; i < len; i++) {
        if (name[i] == '.') {
            dot = (long)i;
        }
    }
    if (dot <= 0) {
        return -1; /* no dot, or a leading dot (hidden file) */
    }
    return dot;
}

bloom_enum_Option_str bloom_path_extension(bloom_str path) {
    bloom_enum_Option_str name = bloom_path_file_name(path);
    if (name.tag != 0) {
        return name;
    }
    bloom_str n = name.data.m_Some._0;
    long dot = bloom_path_ext_dot(n.data, n.len);
    if (dot < 0) {
        bloom_str_drop(&n);
        return bloom_path_none();
    }
    bloom_str out = bloom_str_from_bytes(n.data + dot + 1, n.len - (size_t)dot - 1);
    bloom_str_drop(&n);
    return bloom_path_some(out);
}

bloom_enum_Option_str bloom_path_stem(bloom_str path) {
    bloom_enum_Option_str name = bloom_path_file_name(path);
    if (name.tag != 0) {
        return name;
    }
    bloom_str n = name.data.m_Some._0;
    long dot = bloom_path_ext_dot(n.data, n.len);
    size_t take = dot < 0 ? n.len : (size_t)dot;
    bloom_str out = bloom_str_from_bytes(n.data, take);
    bloom_str_drop(&n);
    return bloom_path_some(out);
}

bool bloom_path_is_absolute(bloom_str path) {
    bloom_path_split s;
    const char *err = bloom_path_split_path(path.data, path.len, &s);
    if (err != NULL) {
        return false;
    }
    bool absolute = s.has_prefix && s.has_root;
    bloom_path_split_free(&s);
    return absolute;
}

bloom_enum_Result_str_str bloom_path_normalize(bloom_str path) {
    if (path.len == 0) {
        return bloom_path_err_str(bloom_path_err("normalize", "empty path"));
    }
    bloom_path_split s;
    const char *err = bloom_path_split_path(path.data, path.len, &s);
    if (err != NULL) {
        return bloom_path_err_str(bloom_path_err("normalize", err));
    }
    bloom_path_span *out = (bloom_path_span *)malloc(
        (s.comp_count ? s.comp_count : 1) * sizeof *out
    );
    if (out == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t count = 0;
    for (size_t i = 0; i < s.comp_count; i++) {
        bloom_path_span c = s.comps[i];
        if (bloom_path_comp_is(&c, ".")) {
            continue;
        }
        if (bloom_path_comp_is(&c, "..")) {
            if (count > 0 && !bloom_path_comp_is(&out[count - 1], "..")) {
                count--;
                continue;
            }
            if (s.has_root) {
                /* `..` cannot climb above a root; drop it. */
                continue;
            }
            out[count++] = c;
            continue;
        }
        out[count++] = c;
    }
    bloom_str result = bloom_path_rebuild(&s, out, count);
    free(out);
    bloom_path_split_free(&s);
    return bloom_path_ok_str(result);
}

bloom_str bloom_path_with_extension(bloom_str path, bloom_str extension) {
    bloom_path_split s;
    const char *err = bloom_path_split_path(path.data, path.len, &s);
    if (err != NULL || s.comp_count == 0) {
        bloom_path_split_free(&s);
        return bloom_str_from_bytes(path.data, path.len);
    }
    /* Drop trailing `.` components; `..` is not a file name. */
    while (s.comp_count > 0 && bloom_path_comp_is(&s.comps[s.comp_count - 1], ".")) {
        s.comp_count--;
    }
    if (s.comp_count == 0 || bloom_path_comp_is(&s.comps[s.comp_count - 1], "..")) {
        bloom_path_split_free(&s);
        return bloom_str_from_bytes(path.data, path.len);
    }
    bloom_path_span name = s.comps[s.comp_count - 1];
    long dot = bloom_path_ext_dot(name.p, name.len);
    size_t stem_len = dot < 0 ? name.len : (size_t)dot;
    /* A single leading dot in the argument is accepted and dropped. */
    size_t ext_at = 0;
    size_t ext_len = extension.len;
    if (ext_len > 0 && extension.data[0] == '.') {
        ext_at = 1;
        ext_len--;
    }
    size_t new_len = stem_len + (ext_len > 0 ? 1 + ext_len : 0);
    char *new_name = (char *)malloc(new_len ? new_len : 1);
    if (new_name == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(new_name, name.p, stem_len);
    size_t at = stem_len;
    if (ext_len > 0) {
        new_name[at++] = '.';
        memcpy(new_name + at, extension.data + ext_at, ext_len);
        at += ext_len;
    }
    bloom_path_span replacement;
    replacement.p = new_name;
    replacement.len = at;
    /* Rebuild with the last component replaced. */
    size_t total = s.prefix_len + (s.has_root ? 1 : 0);
    for (size_t i = 0; i < s.comp_count - 1; i++) {
        total += s.comps[i].len + (i > 0 ? 1 : 0);
    }
    if (s.comp_count > 1) {
        total += 1;
    }
    total += at;
    char *buf = (char *)malloc(total + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t out = 0;
    memcpy(buf + out, s.prefix, s.prefix_len);
    out += s.prefix_len;
    if (s.has_root) {
        buf[out++] = bloom_path_sep();
    }
    for (size_t i = 0; i < s.comp_count - 1; i++) {
        if (i > 0) {
            buf[out++] = bloom_path_sep();
        }
        memcpy(buf + out, s.comps[i].p, s.comps[i].len);
        out += s.comps[i].len;
    }
    if (s.comp_count > 1) {
        buf[out++] = bloom_path_sep();
    }
    memcpy(buf + out, replacement.p, replacement.len);
    out += replacement.len;
    bloom_str result = bloom_str_from_bytes(buf, out);
    free(buf);
    free(new_name);
    bloom_path_split_free(&s);
    return result;
}
