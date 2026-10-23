/* Bloom std::db runtime â€” SQLite 3.54.1 (vendored amalgamation).
 *
 * Design notes:
 *   - One prepared statement per `prepare` call; trailing SQL that is not
 *     whitespace or a comment is rejected. No SQL string interpolation.
 *   - Bind indices are 1-based (SQLite's own numbering: 1..parameter_count).
 *   - Column indices are 0-based (SQLite's own numbering: 0..column_count-1).
 *   - Statement states: 0 fresh (prepared or reset), 1 row (last step
 *     returned a row), 2 done (last step/execute finished), 3 error.
 *     Bind only in state 0; columns only in state 1; step in 0/1;
 *     execute only in 0; reset from any state clears bindings.
 *   - A statement keeps its database connection alive (reference count),
 *     so a Bloom handle can outlive the original `Database` variable.
 *   - Errors come from SQLite but are prefixed with the operation and the
 *     SQLite result code. SQL text is never echoed back.
 *
 * Limits: parameter index up to SQLITE_MAX_VARIABLE_NUMBER, text/blob up to
 * SQLite's 1 GiB object limit, busy timeout 0..60000 ms (default 5000).
 */

#include "sqlite3.h"

#define BLOOM_DB_DEFAULT_BUSY_MS 5000
#define BLOOM_DB_MAX_BUSY_MS 60000

typedef struct {
    size_t rc;
    sqlite3 *db;
} bloom_db;

typedef struct {
    size_t rc;
    sqlite3_stmt *stmt;
    bloom_db *db;
    int state; /* 0 fresh, 1 row, 2 done, 3 error */
} bloom_db_stmt;

static bloom_str bloom_db_make_msg(const char *op, const char *detail) {
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

static bloom_str bloom_db_make_msg2(
    const char *op,
    const char *detail,
    int code
) {
    char buf[512];
    int n = snprintf(
        buf,
        sizeof buf,
        "%s: %s (SQLite code %d)",
        op,
        detail,
        code
    );
    if (n < 0) {
        return bloom_db_make_msg(op, detail);
    }
    size_t len = (size_t)n;
    if (len >= sizeof buf) {
        len = sizeof buf - 1;
    }
    return bloom_str_from_bytes(buf, len);
}

static bloom_str bloom_db_error_text(bloom_db *d, const char *op) {
    return bloom_db_make_msg2(op, sqlite3_errmsg(d->db), sqlite3_errcode(d->db));
}

static bloom_enum_Result_bool_str bloom_db_ok_bool(bool v) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_bool_str bloom_db_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_i64_str bloom_db_ok_int(int64_t v) {
    bloom_enum_Result_i64_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_i64_str bloom_db_err_int(bloom_str m) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_str_str bloom_db_ok_str(bloom_str v) {
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_str_str bloom_db_err_str(bloom_str m) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_f64_str bloom_db_ok_float(double v) {
    bloom_enum_Result_f64_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_f64_str bloom_db_err_float(bloom_str m) {
    bloom_enum_Result_f64_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

/* ---- handles, lifetime -------------------------------------------------- */

static bloom_struct_std_db_Database bloom_db_wrap(bloom_db *d) {
    bloom_struct_std_db_Database v;
    v.f_id = (bloom_db_handle)(intptr_t)d;
    return v;
}

static bloom_db *bloom_db_unwrap(bloom_struct_std_db_Database v) {
    return (bloom_db *)(intptr_t)v.f_id;
}

static bloom_struct_std_db_Statement bloom_db_stmt_wrap(bloom_db_stmt *s) {
    bloom_struct_std_db_Statement v;
    v.f_id = (bloom_stmt_handle)(intptr_t)s;
    return v;
}

static bloom_db_stmt *bloom_db_stmt_unwrap(bloom_struct_std_db_Statement v) {
    return (bloom_db_stmt *)(intptr_t)v.f_id;
}

bloom_db_handle bloom_db_retain(bloom_db_handle h) {
    bloom_db *d = (bloom_db *)(intptr_t)h;
    if (d != NULL) {
        d->rc++;
    }
    return h;
}

static void bloom_db_free(bloom_db *d) {
    sqlite3_close_v2(d->db); /* rolls back an open transaction */
    free(d);
    bloom_live_allocs--;
}

void bloom_db_release(bloom_db_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_db *d = (bloom_db *)(intptr_t)*h;
    *h = 0;
    if (d->rc > 0) {
        d->rc--;
    }
    if (d->rc == 0) {
        bloom_db_free(d);
    }
}

bloom_stmt_handle bloom_db_stmt_retain(bloom_stmt_handle h) {
    bloom_db_stmt *s = (bloom_db_stmt *)(intptr_t)h;
    if (s != NULL) {
        s->rc++;
    }
    return h;
}

void bloom_db_stmt_release(bloom_stmt_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_db_stmt *s = (bloom_db_stmt *)(intptr_t)*h;
    *h = 0;
    if (s->rc > 0) {
        s->rc--;
    }
    if (s->rc == 0) {
        sqlite3_finalize(s->stmt);
        bloom_db_handle dh = (bloom_db_handle)(intptr_t)s->db;
        bloom_db_release(&dh);
        free(s);
        bloom_live_allocs--;
    }
}

/* ---- opening ------------------------------------------------------------ */

/* SQLite takes NUL-terminated UTF-8 paths; embedded NULs would truncate. */
static bool bloom_db_path_ok(bloom_str path) {
    if (path.len == 0) {
        return false;
    }
    for (size_t i = 0; i < path.len; i++) {
        if (path.data[i] == '\0') {
            return false;
        }
    }
    return true;
}

static bloom_enum_Result_std_db_Database_str bloom_db_open_impl(
    bloom_str path,
    const char *op
) {
    if (!bloom_db_path_ok(path)) {
        bloom_enum_Result_std_db_Database_str r;
        r.tag = 1;
        r.data.m_Err._0 = bloom_db_make_msg(
            op,
            "invalid database path (empty or contains a NUL byte)"
        );
        return r;
    }
    char *cpath = (char *)malloc(path.len + 1);
    if (cpath == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(cpath, path.data, path.len);
    cpath[path.len] = '\0';
    sqlite3 *raw = NULL;
    int rc = sqlite3_open_v2(
        cpath,
        &raw,
        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_NOMUTEX,
        NULL
    );
    free(cpath);
    if (rc != SQLITE_OK) {
        bloom_enum_Result_std_db_Database_str r;
        r.tag = 1;
        r.data.m_Err._0 = raw != NULL
            ? bloom_db_make_msg2(op, sqlite3_errmsg(raw), rc)
            : bloom_db_make_msg(op, "could not open database");
        if (raw != NULL) {
            sqlite3_close_v2(raw);
        }
        return r;
    }
    sqlite3_busy_timeout(raw, BLOOM_DB_DEFAULT_BUSY_MS);
    bloom_db *d = (bloom_db *)calloc(1, sizeof(bloom_db));
    if (d == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    d->rc = 1;
    d->db = raw;
    bloom_live_allocs++;
    bloom_enum_Result_std_db_Database_str r;
    r.tag = 0;
    r.data.m_Ok._0 = bloom_db_wrap(d);
    return r;
}

bloom_enum_Result_std_db_Database_str bloom_db_open(bloom_str path) {
    return bloom_db_open_impl(path, "open");
}

bloom_enum_Result_std_db_Database_str bloom_db_open_memory(void) {
    bloom_str name = bloom_str_from_bytes(":memory:", 8);
    bloom_enum_Result_std_db_Database_str r =
        bloom_db_open_impl(name, "open_memory");
    bloom_str_drop(&name);
    return r;
}

bloom_enum_Result_bool_str bloom_db_busy_timeout(
    bloom_struct_std_db_Database database,
    int64_t ms
) {
    bloom_db *d = bloom_db_unwrap(database);
    if (d == NULL) {
        return bloom_db_err_bool(bloom_db_make_msg("busy_timeout", "invalid database"));
    }
    if (ms < 0 || ms > BLOOM_DB_MAX_BUSY_MS) {
        return bloom_db_err_bool(bloom_db_make_msg(
            "busy_timeout",
            "timeout out of range (0..60000 ms)"
        ));
    }
    sqlite3_busy_timeout(d->db, (int)ms);
    return bloom_db_ok_bool(true);
}

/* Only whitespace, `;` and SQL comments may follow the first statement. */
static bool bloom_db_only_trivia(const char *p, const char *end) {
    while (p < end) {
        char c = *p;
        if (c == ' ' || c == '\t' || c == '\r' || c == '\n' || c == ';') {
            p++;
            continue;
        }
        if (c == '-' && p + 1 < end && p[1] == '-') {
            p += 2;
            while (p < end && *p != '\n') {
                p++;
            }
            continue;
        }
        if (c == '/' && p + 1 < end && p[1] == '*') {
            p += 2;
            while (p + 1 < end && !(p[0] == '*' && p[1] == '/')) {
                p++;
            }
            if (p + 1 >= end) {
                return false; /* unterminated comment */
            }
            p += 2;
            continue;
        }
        return false;
    }
    return true;
}

/* ---- preparing and binding --------------------------------------------- */

bloom_enum_Result_std_db_Statement_str bloom_db_prepare(
    bloom_struct_std_db_Database database,
    bloom_str sql
) {
    bloom_enum_Result_std_db_Statement_str r;
    bloom_db *d = bloom_db_unwrap(database);
    if (d == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_db_make_msg("prepare", "invalid database");
        return r;
    }
    if (sql.len == 0) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_db_make_msg("prepare", "empty SQL statement");
        return r;
    }
    if (sql.len > (size_t)INT32_MAX) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_db_make_msg("prepare", "SQL text is too large");
        return r;
    }
    for (size_t i = 0; i < sql.len; i++) {
        if (sql.data[i] == '\0') {
            r.tag = 1;
            r.data.m_Err._0 = bloom_db_make_msg(
                "prepare",
                "SQL text contains a NUL byte"
            );
            return r;
        }
    }
    sqlite3_stmt *stmt = NULL;
    const char *tail = NULL;
    int rc = sqlite3_prepare_v2(
        d->db,
        sql.data,
        (int)sql.len,
        &stmt,
        &tail
    );
    if (rc != SQLITE_OK) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_db_error_text(d, "prepare");
        return r;
    }
    if (stmt == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_db_make_msg("prepare", "SQL contains no statement");
        return r;
    }
    if (tail != NULL
        && !bloom_db_only_trivia(tail, sql.data + sql.len)) {
        sqlite3_finalize(stmt);
        r.tag = 1;
        r.data.m_Err._0 = bloom_db_make_msg(
            "prepare",
            "only one SQL statement per prepare is allowed"
        );
        return r;
    }
    bloom_db_stmt *s = (bloom_db_stmt *)calloc(1, sizeof(bloom_db_stmt));
    if (s == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    s->rc = 1;
    s->stmt = stmt;
    s->db = d;
    d->rc++;
    s->state = 0;
    bloom_live_allocs++;
    r.tag = 0;
    r.data.m_Ok._0 = bloom_db_stmt_wrap(s);
    return r;
}

/* Shared validation for binding in the fresh state. */
static bloom_str bloom_db_bind_check(
    bloom_db_stmt *s,
    int64_t index,
    const char *op
) {
    if (s == NULL) {
        return bloom_db_make_msg(op, "invalid statement");
    }
    if (s->state != 0) {
        return bloom_db_make_msg(
            op,
            "bind is only allowed in the fresh state (call reset first)"
        );
    }
    int count = sqlite3_bind_parameter_count(s->stmt);
    if (index < 1 || index > (int64_t)count) {
        char buf[96];
        snprintf(
            buf,
            sizeof buf,
            "parameter index out of range (1..%d)",
            count
        );
        return bloom_db_make_msg(op, buf);
    }
    return (bloom_str){0, NULL, 0};
}

static bool bloom_db_bind_failed(bloom_db_stmt *s, int rc, bloom_str *err, const char *op) {
    if (rc == SQLITE_OK) {
        return false;
    }
    *err = bloom_db_error_text(s->db, op);
    return true;
}

bloom_enum_Result_bool_str bloom_stmt_bind_null(
    bloom_struct_std_db_Statement statement,
    int64_t index
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err = bloom_db_bind_check(s, index, "bind_null");
    if (err.data != NULL) {
        return bloom_db_err_bool(err);
    }
    int rc = sqlite3_bind_null(s->stmt, (int)index);
    if (bloom_db_bind_failed(s, rc, &err, "bind_null")) {
        return bloom_db_err_bool(err);
    }
    return bloom_db_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_stmt_bind_int(
    bloom_struct_std_db_Statement statement,
    int64_t index,
    int64_t value
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err = bloom_db_bind_check(s, index, "bind_int");
    if (err.data != NULL) {
        return bloom_db_err_bool(err);
    }
    int rc = sqlite3_bind_int64(s->stmt, (int)index, (sqlite3_int64)value);
    if (bloom_db_bind_failed(s, rc, &err, "bind_int")) {
        return bloom_db_err_bool(err);
    }
    return bloom_db_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_stmt_bind_float(
    bloom_struct_std_db_Statement statement,
    int64_t index,
    double value
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err = bloom_db_bind_check(s, index, "bind_float");
    if (err.data != NULL) {
        return bloom_db_err_bool(err);
    }
    int rc = sqlite3_bind_double(s->stmt, (int)index, value);
    if (bloom_db_bind_failed(s, rc, &err, "bind_float")) {
        return bloom_db_err_bool(err);
    }
    return bloom_db_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_stmt_bind_bool(
    bloom_struct_std_db_Statement statement,
    int64_t index,
    bool value
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err = bloom_db_bind_check(s, index, "bind_bool");
    if (err.data != NULL) {
        return bloom_db_err_bool(err);
    }
    int rc = sqlite3_bind_int(s->stmt, (int)index, value ? 1 : 0);
    if (bloom_db_bind_failed(s, rc, &err, "bind_bool")) {
        return bloom_db_err_bool(err);
    }
    return bloom_db_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_stmt_bind_text(
    bloom_struct_std_db_Statement statement,
    int64_t index,
    bloom_str value
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err = bloom_db_bind_check(s, index, "bind_text");
    if (err.data != NULL) {
        return bloom_db_err_bool(err);
    }
    if (value.len > (size_t)INT32_MAX) {
        return bloom_db_err_bool(bloom_db_make_msg(
            "bind_text",
            "text value is too large"
        ));
    }
    int rc = sqlite3_bind_text(
        s->stmt,
        (int)index,
        value.data != NULL ? value.data : "",
        (int)value.len,
        SQLITE_TRANSIENT
    );
    if (bloom_db_bind_failed(s, rc, &err, "bind_text")) {
        return bloom_db_err_bool(err);
    }
    return bloom_db_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_stmt_bind_bytes(
    bloom_struct_std_db_Statement statement,
    int64_t index,
    bloom_struct_std_fs_Bytes value
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err = bloom_db_bind_check(s, index, "bind_bytes");
    if (err.data != NULL) {
        return bloom_db_err_bool(err);
    }
    bloom_fs_bytes *b = bloom_fs_bytes_unwrap(value);
    if (b == NULL) {
        return bloom_db_err_bool(bloom_db_make_msg(
            "bind_bytes",
            "invalid byte buffer"
        ));
    }
    if (b->len > (size_t)INT32_MAX) {
        return bloom_db_err_bool(bloom_db_make_msg(
            "bind_bytes",
            "blob value is too large"
        ));
    }
    const void *data = b->len > 0 ? (const void *)b->data : (const void *)"";
    int rc = sqlite3_bind_blob(
        s->stmt,
        (int)index,
        data,
        (int)b->len,
        SQLITE_TRANSIENT
    );
    if (bloom_db_bind_failed(s, rc, &err, "bind_bytes")) {
        return bloom_db_err_bool(err);
    }
    return bloom_db_ok_bool(true);
}

/* ---- execution ---------------------------------------------------------- */

bloom_enum_Result_bool_str bloom_stmt_step(
    bloom_struct_std_db_Statement statement
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    if (s == NULL) {
        return bloom_db_err_bool(bloom_db_make_msg("step", "invalid statement"));
    }
    if (s->state == 2) {
        return bloom_db_err_bool(bloom_db_make_msg(
            "step",
            "statement is done; call reset first"
        ));
    }
    if (s->state == 3) {
        return bloom_db_err_bool(bloom_db_make_msg(
            "step",
            "statement is in an error state; call reset first"
        ));
    }
    int rc = sqlite3_step(s->stmt);
    if (rc == SQLITE_ROW) {
        s->state = 1;
        return bloom_db_ok_bool(true);
    }
    if (rc == SQLITE_DONE) {
        s->state = 2;
        return bloom_db_ok_bool(false);
    }
    s->state = 3;
    return bloom_db_err_bool(bloom_db_error_text(s->db, "step"));
}

bloom_enum_Result_i64_str bloom_stmt_execute(
    bloom_struct_std_db_Statement statement
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    if (s == NULL) {
        return bloom_db_err_int(bloom_db_make_msg("execute", "invalid statement"));
    }
    if (s->state != 0) {
        return bloom_db_err_int(bloom_db_make_msg(
            "execute",
            "execute requires the fresh state (call reset first)"
        ));
    }
    int rc = sqlite3_step(s->stmt);
    if (rc == SQLITE_DONE) {
        s->state = 2;
        return bloom_db_ok_int((int64_t)sqlite3_changes(s->db->db));
    }
    if (rc == SQLITE_ROW) {
        s->state = 3;
        return bloom_db_err_int(bloom_db_make_msg(
            "execute",
            "statement returns rows; use step instead"
        ));
    }
    s->state = 3;
    return bloom_db_err_int(bloom_db_error_text(s->db, "execute"));
}

bloom_enum_Result_bool_str bloom_stmt_reset(
    bloom_struct_std_db_Statement statement
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    if (s == NULL) {
        return bloom_db_err_bool(bloom_db_make_msg("reset", "invalid statement"));
    }
    sqlite3_reset(s->stmt);
    sqlite3_clear_bindings(s->stmt);
    s->state = 0;
    return bloom_db_ok_bool(true);
}

/* ---- reading columns ---------------------------------------------------- */

static const char *bloom_db_type_name(int type) {
    switch (type) {
        case SQLITE_INTEGER: return "integer";
        case SQLITE_FLOAT: return "float";
        case SQLITE_TEXT: return "text";
        case SQLITE_BLOB: return "blob";
        case SQLITE_NULL: return "null";
        default: return "unknown";
    }
}

static bloom_str bloom_db_column_error(int64_t index, const char *got, const char *want) {
    char buf[160];
    snprintf(
        buf,
        sizeof buf,
        "column %" PRId64 " is %s, expected %s",
        index,
        got,
        want
    );
    return bloom_db_make_msg("column", buf);
}

static bool bloom_db_column_check(
    bloom_db_stmt *s,
    int64_t index,
    bloom_str *err,
    const char *op
) {
    if (s == NULL) {
        *err = bloom_db_make_msg(op, "invalid statement");
        return false;
    }
    if (s->state != 1) {
        *err = bloom_db_make_msg(
            op,
            "columns are only available while a row is active (call step)"
        );
        return false;
    }
    int count = sqlite3_column_count(s->stmt);
    if (index < 0 || index >= (int64_t)count) {
        char buf[96];
        snprintf(buf, sizeof buf, "column index out of range (0..%d)", count - 1);
        *err = bloom_db_make_msg(op, buf);
        return false;
    }
    return true;
}

int64_t bloom_stmt_column_count(bloom_struct_std_db_Statement statement) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    if (s == NULL) {
        return 0;
    }
    return (int64_t)sqlite3_column_count(s->stmt);
}

bloom_enum_Result_str_str bloom_stmt_column_name(
    bloom_struct_std_db_Statement statement,
    int64_t index
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    if (s == NULL) {
        return bloom_db_err_str(bloom_db_make_msg("column_name", "invalid statement"));
    }
    int count = sqlite3_column_count(s->stmt);
    if (index < 0 || index >= (int64_t)count) {
        char buf[96];
        snprintf(buf, sizeof buf, "column index out of range (0..%d)", count - 1);
        return bloom_db_err_str(bloom_db_make_msg("column_name", buf));
    }
    const char *name = sqlite3_column_name(s->stmt, (int)index);
    if (name == NULL) {
        return bloom_db_err_str(bloom_db_make_msg("column_name", "no column name"));
    }
    return bloom_db_ok_str(bloom_str_from_bytes(name, strlen(name)));
}

/* Unique column name to index; duplicates are rejected as ambiguous. */
bloom_enum_Result_i64_str bloom_stmt_column_index(
    bloom_struct_std_db_Statement statement,
    bloom_str name
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    if (s == NULL) {
        return bloom_db_err_int(bloom_db_make_msg("column_index", "invalid statement"));
    }
    int count = sqlite3_column_count(s->stmt);
    int64_t found = -1;
    int hits = 0;
    for (int i = 0; i < count; i++) {
        const char *col = sqlite3_column_name(s->stmt, i);
        size_t len = col != NULL ? strlen(col) : 0;
        if (len == name.len && (len == 0 || memcmp(col, name.data, len) == 0)) {
            hits++;
            if (hits == 1) {
                found = i;
            }
        }
    }
    if (hits == 0) {
        return bloom_db_err_int(bloom_db_make_msg("column_index", "column not found"));
    }
    if (hits > 1) {
        return bloom_db_err_int(bloom_db_make_msg(
            "column_index",
            "column name is ambiguous"
        ));
    }
    return bloom_db_ok_int(found);
}

bloom_enum_Result_str_str bloom_stmt_column_type(
    bloom_struct_std_db_Statement statement,
    int64_t index
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err;
    if (!bloom_db_column_check(s, index, &err, "column_type")) {
        return bloom_db_err_str(err);
    }
    const char *name = bloom_db_type_name(sqlite3_column_type(s->stmt, (int)index));
    return bloom_db_ok_str(bloom_str_from_bytes(name, strlen(name)));
}

bloom_enum_Result_i64_str bloom_stmt_column_int(
    bloom_struct_std_db_Statement statement,
    int64_t index
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err;
    if (!bloom_db_column_check(s, index, &err, "column_int")) {
        return bloom_db_err_int(err);
    }
    int type = sqlite3_column_type(s->stmt, (int)index);
    if (type != SQLITE_INTEGER) {
        return bloom_db_err_int(bloom_db_column_error(
            index,
            bloom_db_type_name(type),
            "integer"
        ));
    }
    return bloom_db_ok_int((int64_t)sqlite3_column_int64(s->stmt, (int)index));
}

bloom_enum_Result_f64_str bloom_stmt_column_float(
    bloom_struct_std_db_Statement statement,
    int64_t index
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err;
    if (!bloom_db_column_check(s, index, &err, "column_float")) {
        return bloom_db_err_float(err);
    }
    int type = sqlite3_column_type(s->stmt, (int)index);
    if (type != SQLITE_FLOAT) {
        return bloom_db_err_float(bloom_db_column_error(
            index,
            bloom_db_type_name(type),
            "float"
        ));
    }
    return bloom_db_ok_float(sqlite3_column_double(s->stmt, (int)index));
}

bloom_enum_Result_bool_str bloom_stmt_column_bool(
    bloom_struct_std_db_Statement statement,
    int64_t index
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err;
    if (!bloom_db_column_check(s, index, &err, "column_bool")) {
        return bloom_db_err_bool(err);
    }
    int type = sqlite3_column_type(s->stmt, (int)index);
    if (type != SQLITE_INTEGER) {
        return bloom_db_err_bool(bloom_db_column_error(
            index,
            bloom_db_type_name(type),
            "boolean (integer 0 or 1)"
        ));
    }
    sqlite3_int64 v = sqlite3_column_int64(s->stmt, (int)index);
    if (v != 0 && v != 1) {
        return bloom_db_err_bool(bloom_db_column_error(
            index,
            "integer other than 0 or 1",
            "boolean"
        ));
    }
    return bloom_db_ok_bool(v == 1);
}

bloom_enum_Result_str_str bloom_stmt_column_text(
    bloom_struct_std_db_Statement statement,
    int64_t index
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err;
    if (!bloom_db_column_check(s, index, &err, "column_text")) {
        return bloom_db_err_str(err);
    }
    int type = sqlite3_column_type(s->stmt, (int)index);
    if (type != SQLITE_TEXT) {
        return bloom_db_err_str(bloom_db_column_error(
            index,
            bloom_db_type_name(type),
            "text"
        ));
    }
    const unsigned char *data = sqlite3_column_text(s->stmt, (int)index);
    int len = sqlite3_column_bytes(s->stmt, (int)index);
    return bloom_db_ok_str(bloom_str_from_bytes(
        data != NULL ? (const char *)data : "",
        (size_t)len
    ));
}

bloom_enum_Result_std_fs_Bytes_str bloom_stmt_column_bytes(
    bloom_struct_std_db_Statement statement,
    int64_t index
) {
    bloom_db_stmt *s = bloom_db_stmt_unwrap(statement);
    bloom_str err;
    if (!bloom_db_column_check(s, index, &err, "column_bytes")) {
        bloom_enum_Result_std_fs_Bytes_str r;
        r.tag = 1;
        r.data.m_Err._0 = err;
        return r;
    }
    int type = sqlite3_column_type(s->stmt, (int)index);
    if (type != SQLITE_BLOB) {
        bloom_enum_Result_std_fs_Bytes_str r;
        r.tag = 1;
        r.data.m_Err._0 = bloom_db_column_error(
            index,
            bloom_db_type_name(type),
            "blob"
        );
        return r;
    }
    const void *data = sqlite3_column_blob(s->stmt, (int)index);
    int len = sqlite3_column_bytes(s->stmt, (int)index);
    unsigned char *copy = (unsigned char *)malloc(len > 0 ? (size_t)len : 1);
    if (copy == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    if (len > 0 && data != NULL) {
        memcpy(copy, data, (size_t)len);
    }
    bloom_struct_std_fs_Bytes v = bloom_fs_bytes_take(copy, (size_t)len);
    bloom_enum_Result_std_fs_Bytes_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

/* ---- database-level helpers -------------------------------------------- */

int64_t bloom_db_changes(bloom_struct_std_db_Database database) {
    bloom_db *d = bloom_db_unwrap(database);
    return d == NULL ? 0 : (int64_t)sqlite3_changes(d->db);
}

int64_t bloom_db_last_insert_rowid(bloom_struct_std_db_Database database) {
    bloom_db *d = bloom_db_unwrap(database);
    return d == NULL ? 0 : (int64_t)sqlite3_last_insert_rowid(d->db);
}

static bloom_enum_Result_bool_str bloom_db_exec(
    bloom_struct_std_db_Database database,
    const char *sql,
    const char *op
) {
    bloom_db *d = bloom_db_unwrap(database);
    if (d == NULL) {
        return bloom_db_err_bool(bloom_db_make_msg(op, "invalid database"));
    }
    char *message = NULL;
    int rc = sqlite3_exec(d->db, sql, NULL, NULL, &message);
    if (rc != SQLITE_OK) {
        bloom_enum_Result_bool_str r;
        r.tag = 1;
        r.data.m_Err._0 = bloom_db_make_msg2(
            op,
            message != NULL ? message : sqlite3_errmsg(d->db),
            rc
        );
        if (message != NULL) {
            sqlite3_free(message);
        }
        return r;
    }
    return bloom_db_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_db_begin(bloom_struct_std_db_Database database) {
    return bloom_db_exec(database, "BEGIN", "begin");
}

bloom_enum_Result_bool_str bloom_db_commit(bloom_struct_std_db_Database database) {
    return bloom_db_exec(database, "COMMIT", "commit");
}

bloom_enum_Result_bool_str bloom_db_rollback(bloom_struct_std_db_Database database) {
    return bloom_db_exec(database, "ROLLBACK", "rollback");
}
