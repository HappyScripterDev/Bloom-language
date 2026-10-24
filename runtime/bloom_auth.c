/* Bloom std::auth runtime — password hashing, accounts, roles, sessions and
 * a bounded login rate limiter.
 *
 * Hashing: Argon2id from the vendored reference implementation (20190702,
 * CC0/Apache-2.0, see runtime/argon2/LICENSE and runtime/argon2_NOTICE.md),
 * compiled without threads. Parameters: m=65536 KiB, t=3, p=1, salt 16
 * bytes, tag 32 bytes (RFC 9106 second recommendation, p=1).
 *
 * Hash strings use the PHC format:
 *   $argon2id$v=19$m=65536,t=3,p=1$<salt-base64>$<hash-base64>
 * Verification rejects unknown algorithms and unreasonable parameters
 * (m > 256 MiB, t > 10, p > 4) before calling into Argon2.
 *
 * Accounts live in SQLite (std::db). All user input is bound, never
 * interpolated. Usernames are ASCII-normalized (trimmed, lowercase,
 * 3..32 bytes, [a-z0-9._-], starting with a letter or digit). Passwords are
 * never trimmed or normalized; 8..1024 bytes for new passwords, 0..1024 for
 * verification. No password or hash is ever written to session data.
 *
 * Authenticated sessions store the user id and the account's
 * auth_version under reserved keys. Password changes and
 * enable/disable bump auth_version, which invalidates older sessions on
 * their next `current_user` check. Logout destroys the server-side session.
 */

#include "argon2/include/argon2.h"

#define BLOOM_AUTH_SCHEMA_VERSION "1"
#define BLOOM_AUTH_USERNAME_MIN 3
#define BLOOM_AUTH_USERNAME_MAX 32
#define BLOOM_AUTH_PASSWORD_MIN 8
#define BLOOM_AUTH_PASSWORD_MAX 1024
#define BLOOM_AUTH_HASH_MAX 512
#define BLOOM_AUTH_SALT_BYTES 16
#define BLOOM_AUTH_TAG_BYTES 32
#define BLOOM_AUTH_M_COST 65536
#define BLOOM_AUTH_T_COST 3
#define BLOOM_AUTH_P_COST 1
#define BLOOM_AUTH_M_COST_MAX 262144
#define BLOOM_AUTH_T_COST_MAX 10
#define BLOOM_AUTH_P_COST_MAX 4
#define BLOOM_AUTH_LIMITER_MAX_ENTRIES 65536
#define BLOOM_AUTH_LIMITER_MAX_WINDOW 86400
#define BLOOM_AUTH_LIMITER_MAX_ATTEMPTS 1000
#define BLOOM_AUTH_LIMITER_KEY_MAX 256

/* A syntactically valid Argon2id hash with the production parameters whose
 * tag never matches a real password. Used to spend the same work for
 * unknown usernames as for known ones. */
#define BLOOM_AUTH_DUMMY_HASH \
    "$argon2id$v=19$m=65536,t=3,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"

static bloom_str bloom_auth_lit(const char *s, size_t n) {
    bloom_str v;
    v.rc = BLOOM_STR_IMMORTAL;
    v.data = (char *)s;
    v.len = n;
    return v;
}

static bloom_str bloom_auth_key(const char *s) {
    return bloom_auth_lit(s, strlen(s));
}

static bloom_str bloom_auth_msg(const char *op, const char *detail) {
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

static bloom_enum_Result_bool_str bloom_auth_ok_bool(bool v) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_bool_str bloom_auth_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_i64_str bloom_auth_ok_int(int64_t v) {
    bloom_enum_Result_i64_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_i64_str bloom_auth_err_int(bloom_str m) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_str_str bloom_auth_ok_str(bloom_str v) {
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_str_str bloom_auth_err_str(bloom_str m) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_std_auth_UserInfo_str bloom_auth_ok_user(
    bloom_struct_std_auth_UserInfo v
) {
    bloom_enum_Result_std_auth_UserInfo_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_std_auth_UserInfo_str bloom_auth_err_user(bloom_str m) {
    bloom_enum_Result_std_auth_UserInfo_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Option_std_auth_UserInfo bloom_auth_none_user(void) {
    bloom_enum_Option_std_auth_UserInfo r;
    r.tag = 1;
    return r;
}

static bloom_enum_Option_std_auth_UserInfo bloom_auth_some_user(
    bloom_struct_std_auth_UserInfo v
) {
    bloom_enum_Option_std_auth_UserInfo r;
    r.tag = 0;
    r.data.m_Some._0 = v;
    return r;
}

static bloom_enum_Result_Option_std_auth_UserInfo_str bloom_auth_ok_opt(
    bloom_enum_Option_std_auth_UserInfo v
) {
    bloom_enum_Result_Option_std_auth_UserInfo_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_Option_std_auth_UserInfo_str bloom_auth_err_opt(bloom_str m) {
    bloom_enum_Result_Option_std_auth_UserInfo_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bool bloom_auth_contains(bloom_str s, const char *needle) {
    size_t n = strlen(needle);
    if (n == 0 || s.len < n) {
        return false;
    }
    for (size_t i = 0; i + n <= s.len; i++) {
        if (memcmp(s.data + i, needle, n) == 0) {
            return true;
        }
    }
    return false;
}

/* ---- base64 (standard alphabet, no padding) ----------------------------- */

static const char bloom_auth_b64_table[] =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

static size_t bloom_auth_b64_len(size_t n) {
    return (n * 4 + 2) / 3;
}

static void bloom_auth_b64_encode(const unsigned char *in, size_t n, char *out) {
    size_t o = 0;
    size_t i = 0;
    while (i + 2 < n) {
        uint32_t v = ((uint32_t)in[i] << 16) | ((uint32_t)in[i + 1] << 8)
            | (uint32_t)in[i + 2];
        out[o++] = bloom_auth_b64_table[(v >> 18) & 63];
        out[o++] = bloom_auth_b64_table[(v >> 12) & 63];
        out[o++] = bloom_auth_b64_table[(v >> 6) & 63];
        out[o++] = bloom_auth_b64_table[v & 63];
        i += 3;
    }
    if (i < n) {
        uint32_t v = (uint32_t)in[i] << 16;
        if (i + 1 < n) {
            v |= (uint32_t)in[i + 1] << 8;
        }
        out[o++] = bloom_auth_b64_table[(v >> 18) & 63];
        out[o++] = bloom_auth_b64_table[(v >> 12) & 63];
        if (i + 1 < n) {
            out[o++] = bloom_auth_b64_table[(v >> 6) & 63];
        }
    }
}

static bool bloom_auth_b64_valid(const char *s, size_t n) {
    if (n == 0) {
        return false;
    }
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        bool ok = (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z')
            || (c >= '0' && c <= '9') || c == '+' || c == '/';
        if (!ok) {
            return false;
        }
    }
    return true;
}

/* Length in bytes of an unpadded base64 string, or -1 when invalid. */
static int bloom_auth_b64_decoded_len(const char *s, size_t n) {
    if (!bloom_auth_b64_valid(s, n) || n % 4 == 1 || n > 128) {
        return -1;
    }
    return (int)((n * 3) / 4);
}

/* ---- PHC parsing ------------------------------------------------------- */

/* Parses "$argon2id$v=19$m=..,t=..,p=..$salt$hash" with bounded values. */
static bool bloom_auth_parse_phc(
    bloom_str hash,
    int64_t *m_cost,
    int64_t *t_cost,
    int64_t *p_cost,
    int *salt_len,
    int *tag_len
) {
    const char *s = hash.data;
    size_t n = hash.len;
    if (n == 0 || n > BLOOM_AUTH_HASH_MAX) {
        return false;
    }
    const char *prefix = "$argon2id$v=";
    size_t prefix_len = strlen(prefix);
    if (n <= prefix_len || memcmp(s, prefix, prefix_len) != 0) {
        return false;
    }
    size_t i = prefix_len;
    size_t vstart = i;
    while (i < n && s[i] >= '0' && s[i] <= '9') {
        i++;
    }
    if (i == vstart || i >= n || s[i] != '$') {
        return false;
    }
    if (i - vstart != 2 || s[vstart] != '1' || s[vstart + 1] != '9') {
        return false;
    }
    i++;
    if (i + 2 >= n || s[i] != 'm' || s[i + 1] != '=') {
        return false;
    }
    i += 2;
    int64_t m = 0;
    size_t mstart = i;
    while (i < n && s[i] >= '0' && s[i] <= '9') {
        if (m > (INT64_MAX - 9) / 10) {
            return false;
        }
        m = m * 10 + (s[i] - '0');
        i++;
    }
    if (i == mstart || i + 3 >= n || s[i] != ',' || s[i + 1] != 't'
        || s[i + 2] != '=') {
        return false;
    }
    i += 3;
    int64_t t = 0;
    size_t tstart = i;
    while (i < n && s[i] >= '0' && s[i] <= '9') {
        if (t > (INT64_MAX - 9) / 10) {
            return false;
        }
        t = t * 10 + (s[i] - '0');
        i++;
    }
    if (i == tstart || i + 3 >= n || s[i] != ',' || s[i + 1] != 'p'
        || s[i + 2] != '=') {
        return false;
    }
    i += 3;
    int64_t p = 0;
    size_t pstart = i;
    while (i < n && s[i] >= '0' && s[i] <= '9') {
        if (p > 999) {
            return false;
        }
        p = p * 10 + (s[i] - '0');
        i++;
    }
    if (i == pstart || i >= n || s[i] != '$') {
        return false;
    }
    i++;
    size_t salt_start = i;
    while (i < n && s[i] != '$') {
        i++;
    }
    if (i >= n || i == salt_start) {
        return false;
    }
    size_t salt_chars = i - salt_start;
    i++;
    size_t tag_start = i;
    while (i < n && s[i] != '$') {
        i++;
    }
    if (i != n || i == tag_start) {
        return false;
    }
    size_t tag_chars = i - tag_start;
    int salt_bytes = bloom_auth_b64_decoded_len(s + salt_start, salt_chars);
    int tag_bytes = bloom_auth_b64_decoded_len(s + tag_start, tag_chars);
    if (salt_bytes < 8 || salt_bytes > 64 || tag_bytes < 16 || tag_bytes > 64) {
        return false;
    }
    if (m < 8192 || m > BLOOM_AUTH_M_COST_MAX || t < 1
        || t > BLOOM_AUTH_T_COST_MAX || p < 1 || p > BLOOM_AUTH_P_COST_MAX) {
        return false;
    }
    *m_cost = m;
    *t_cost = t;
    *p_cost = p;
    *salt_len = salt_bytes;
    *tag_len = tag_bytes;
    return true;
}

/* ---- password hashing -------------------------------------------------- */

bloom_enum_Result_str_str bloom_auth_password_hash(bloom_str password) {
    if (password.len < BLOOM_AUTH_PASSWORD_MIN) {
        return bloom_auth_err_str(bloom_auth_msg(
            "password_hash",
            "password must be at least 8 bytes"
        ));
    }
    if (password.len > BLOOM_AUTH_PASSWORD_MAX) {
        return bloom_auth_err_str(bloom_auth_msg(
            "password_hash",
            "password must not exceed 1024 bytes"
        ));
    }
    unsigned char salt[BLOOM_AUTH_SALT_BYTES];
    unsigned char tag[BLOOM_AUTH_TAG_BYTES];
    if (!bloom_web_random_bytes(salt, sizeof salt)) {
        return bloom_auth_err_str(bloom_auth_msg(
            "password_hash",
            "secure random source unavailable"
        ));
    }
    int rc = argon2id_hash_raw(
        BLOOM_AUTH_T_COST,
        BLOOM_AUTH_M_COST,
        BLOOM_AUTH_P_COST,
        password.data != NULL ? password.data : "",
        (size_t)password.len,
        salt,
        sizeof salt,
        tag,
        sizeof tag
    );
    if (rc != ARGON2_OK) {
        char buf[160];
        snprintf(
            buf,
            sizeof buf,
            "argon2id failed: %s",
            argon2_error_message(rc)
        );
        return bloom_auth_err_str(bloom_auth_msg("password_hash", buf));
    }
    char b64salt[32];
    char b64tag[64];
    bloom_auth_b64_encode(salt, sizeof salt, b64salt);
    bloom_auth_b64_encode(tag, sizeof tag, b64tag);
    char encoded[192];
    int n = snprintf(
        encoded,
        sizeof encoded,
        "$argon2id$v=19$m=%d,t=%d,p=%d$%.*s$%.*s",
        BLOOM_AUTH_M_COST,
        BLOOM_AUTH_T_COST,
        BLOOM_AUTH_P_COST,
        (int)bloom_auth_b64_len(sizeof salt),
        b64salt,
        (int)bloom_auth_b64_len(sizeof tag),
        b64tag
    );
    if (n <= 0 || (size_t)n >= sizeof encoded) {
        return bloom_auth_err_str(bloom_auth_msg(
            "password_hash",
            "encoded hash is too long"
        ));
    }
    return bloom_auth_ok_str(bloom_str_from_bytes(encoded, (size_t)n));
}

bloom_enum_Result_bool_str bloom_auth_password_verify(
    bloom_str password,
    bloom_str hash
) {
    if (password.len > BLOOM_AUTH_PASSWORD_MAX) {
        return bloom_auth_err_bool(bloom_auth_msg(
            "password_verify",
            "password exceeds 1024 bytes"
        ));
    }
    int64_t m = 0;
    int64_t t = 0;
    int64_t p = 0;
    int salt_len = 0;
    int tag_len = 0;
    if (!bloom_auth_parse_phc(hash, &m, &t, &p, &salt_len, &tag_len)) {
        return bloom_auth_err_bool(bloom_auth_msg(
            "password_verify",
            "invalid or unsupported password hash"
        ));
    }
    char *encoded = (char *)malloc(hash.len + 1);
    if (encoded == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    memcpy(encoded, hash.data, hash.len);
    encoded[hash.len] = '\0';
    int rc = argon2id_verify(encoded, password.data != NULL ? password.data : "", password.len);
    free(encoded);
    if (rc == ARGON2_OK) {
        return bloom_auth_ok_bool(true);
    }
    if (rc == ARGON2_VERIFY_MISMATCH) {
        return bloom_auth_ok_bool(false);
    }
    char buf[160];
    snprintf(
        buf,
        sizeof buf,
        "argon2id failed: %s",
        argon2_error_message(rc)
    );
    return bloom_auth_err_bool(bloom_auth_msg("password_verify", buf));
}

bloom_enum_Result_bool_str bloom_auth_password_needs_rehash(bloom_str hash) {
    int64_t m = 0;
    int64_t t = 0;
    int64_t p = 0;
    int salt_len = 0;
    int tag_len = 0;
    if (!bloom_auth_parse_phc(hash, &m, &t, &p, &salt_len, &tag_len)) {
        return bloom_auth_err_bool(bloom_auth_msg(
            "password_needs_rehash",
            "invalid or unsupported password hash"
        ));
    }
    bool needs = m != BLOOM_AUTH_M_COST || t != BLOOM_AUTH_T_COST
        || p != BLOOM_AUTH_P_COST || salt_len != BLOOM_AUTH_SALT_BYTES
        || tag_len != BLOOM_AUTH_TAG_BYTES;
    return bloom_auth_ok_bool(needs);
}

/* ---- small SQL helpers ------------------------------------------------- */

static bloom_enum_Result_i64_str bloom_auth_exec(
    bloom_struct_std_db_Database database,
    const char *sql
) {
    bloom_str text = bloom_auth_key(sql);
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, text);
    if (prep.tag == 1) {
        return bloom_auth_err_int(prep.data.m_Err._0);
    }
    bloom_enum_Result_i64_str r = bloom_stmt_execute(prep.data.m_Ok._0);
    bloom_db_stmt_release(&prep.data.m_Ok._0.f_id);
    return r;
}

/* Runs `SELECT value FROM bloom_auth_meta WHERE key = ?1`; Ok(None) when the
 * key does not exist. */
static bloom_enum_Result_Option_str_str bloom_auth_meta_get(
    bloom_struct_std_db_Database database,
    const char *key
) {
    bloom_str sql = bloom_auth_key(
        "SELECT value FROM bloom_auth_meta WHERE key = ?1"
    );
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        bloom_enum_Result_Option_str_str r;
        r.tag = 1;
        r.data.m_Err._0 = prep.data.m_Err._0;
        return r;
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_enum_Result_Option_str_str result;
    result.tag = 1;
    result.data.m_Err._0 = (bloom_str){0, NULL, 0};
    bloom_str k = bloom_auth_key(key);
    bloom_enum_Result_bool_str bound = bloom_stmt_bind_text(st, 1, k);
    if (bound.tag == 1) {
        result.data.m_Err._0 = bound.data.m_Err._0;
        bloom_db_stmt_release(&st.f_id);
        return result;
    }
    bloom_enum_Result_bool_str stepped = bloom_stmt_step(st);
    if (stepped.tag == 1) {
        result.data.m_Err._0 = stepped.data.m_Err._0;
        bloom_db_stmt_release(&st.f_id);
        return result;
    }
    if (!stepped.data.m_Ok._0) {
        bloom_db_stmt_release(&st.f_id);
        result.tag = 0;
        result.data.m_Ok._0.tag = 1;
        return result;
    }
    bloom_enum_Result_str_str text = bloom_stmt_column_text(st, 0);
    bloom_db_stmt_release(&st.f_id);
    if (text.tag == 1) {
        result.data.m_Err._0 = text.data.m_Err._0;
        return result;
    }
    result.tag = 0;
    result.data.m_Ok._0.tag = 0;
    result.data.m_Ok._0.data.m_Some._0 = text.data.m_Ok._0;
    return result;
}

static bloom_enum_Result_bool_str bloom_auth_table_exists(
    bloom_struct_std_db_Database database,
    const char *name
) {
    bloom_str sql = bloom_auth_key(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1"
    );
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        return bloom_auth_err_bool(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_str n = bloom_auth_key(name);
    bloom_enum_Result_bool_str bound = bloom_stmt_bind_text(st, 1, n);
    if (bound.tag == 1) {
        bloom_enum_Result_bool_str r = bound;
        bloom_db_stmt_release(&st.f_id);
        return r;
    }
    bloom_enum_Result_bool_str stepped = bloom_stmt_step(st);
    if (stepped.tag == 1) {
        bloom_enum_Result_bool_str r = stepped;
        bloom_db_stmt_release(&st.f_id);
        return r;
    }
    if (!stepped.data.m_Ok._0) {
        bloom_db_stmt_release(&st.f_id);
        return bloom_auth_ok_bool(false);
    }
    bloom_enum_Result_i64_str count = bloom_stmt_column_int(st, 0);
    bloom_db_stmt_release(&st.f_id);
    if (count.tag == 1) {
        return bloom_auth_err_bool(count.data.m_Err._0);
    }
    return bloom_auth_ok_bool(count.data.m_Ok._0 > 0);
}

/* ---- schema ------------------------------------------------------------ */

bloom_enum_Result_bool_str bloom_auth_setup(bloom_struct_std_db_Database database) {
    bloom_enum_Result_bool_str meta = bloom_auth_table_exists(database, "bloom_auth_meta");
    if (meta.tag == 1) {
        return meta;
    }
    if (meta.data.m_Ok._0) {
        bloom_enum_Result_Option_str_str version = bloom_auth_meta_get(
            database,
            "schema_version"
        );
        if (version.tag == 1) {
            return bloom_auth_err_bool(version.data.m_Err._0);
        }
        if (version.data.m_Ok._0.tag == 1) {
            return bloom_auth_err_bool(bloom_auth_msg(
                "setup",
                "auth schema metadata is incomplete"
            ));
        }
        bloom_str v = version.data.m_Ok._0.data.m_Some._0;
        bool current = v.len == 1 && v.data[0] == '1';
        bloom_str_drop(&v);
        if (!current) {
            return bloom_auth_err_bool(bloom_auth_msg(
                "setup",
                "unsupported auth schema version (expected 1)"
            ));
        }
        return bloom_auth_ok_bool(true);
    }
    bloom_enum_Result_bool_str users = bloom_auth_table_exists(
        database,
        "bloom_auth_users"
    );
    if (users.tag == 1) {
        return users;
    }
    if (users.data.m_Ok._0) {
        return bloom_auth_err_bool(bloom_auth_msg(
            "setup",
            "existing auth tables are not managed by std::auth"
        ));
    }
    bloom_enum_Result_bool_str begun = bloom_db_begin(database);
    if (begun.tag == 1) {
        return begun;
    }
    bloom_enum_Result_i64_str create = bloom_auth_exec(
        database,
        "CREATE TABLE bloom_auth_meta ("
        "key TEXT PRIMARY KEY, value TEXT NOT NULL)"
    );
    if (create.tag == 0) {
        create = bloom_auth_exec(
            database,
            "CREATE TABLE bloom_auth_users ("
            "id INTEGER PRIMARY KEY AUTOINCREMENT,"
            "username TEXT NOT NULL UNIQUE,"
            "display_name TEXT NOT NULL DEFAULT '',"
            "password_hash TEXT NOT NULL,"
            "active INTEGER NOT NULL DEFAULT 1,"
            "auth_version INTEGER NOT NULL DEFAULT 1,"
            "created_at INTEGER NOT NULL)"
        );
    }
    if (create.tag == 0) {
        create = bloom_auth_exec(
            database,
            "CREATE TABLE bloom_auth_roles (name TEXT PRIMARY KEY)"
        );
    }
    if (create.tag == 0) {
        create = bloom_auth_exec(
            database,
            "CREATE TABLE bloom_auth_user_roles ("
            "user_id INTEGER NOT NULL,"
            "role TEXT NOT NULL,"
            "PRIMARY KEY (user_id, role))"
        );
    }
    if (create.tag == 0) {
        bloom_str sql = bloom_auth_key(
            "INSERT INTO bloom_auth_meta (key, value) VALUES (?1, ?2)"
        );
        bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
        if (prep.tag == 1) {
            create.tag = 1;
            create.data.m_Err._0 = prep.data.m_Err._0;
        } else {
            bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
            bloom_str k = bloom_auth_key("schema_version");
            bloom_str v = bloom_auth_key(BLOOM_AUTH_SCHEMA_VERSION);
            bloom_enum_Result_bool_str b1 = bloom_stmt_bind_text(st, 1, k);
            bloom_enum_Result_bool_str b2 = b1.tag == 0
                ? bloom_stmt_bind_text(st, 2, v)
                : b1;
            if (b1.tag == 1) {
                create.tag = 1;
                create.data.m_Err._0 = b1.data.m_Err._0;
            } else if (b2.tag == 1) {
                create.tag = 1;
                create.data.m_Err._0 = b2.data.m_Err._0;
            } else {
                create = bloom_stmt_execute(st);
            }
            bloom_db_stmt_release(&st.f_id);
        }
    }
    if (create.tag == 1) {
        bloom_str e = create.data.m_Err._0;
        bloom_db_rollback(database);
        return bloom_auth_err_bool(e);
    }
    bloom_enum_Result_bool_str committed = bloom_db_commit(database);
    if (committed.tag == 1) {
        bloom_db_rollback(database);
        return committed;
    }
    return bloom_auth_ok_bool(true);
}

/* ---- usernames --------------------------------------------------------- */

static void bloom_auth_trimmed(bloom_str in, size_t *start, size_t *len) {
    size_t s = 0;
    size_t e = in.len;
    while (s < e && (in.data[s] == ' ' || in.data[s] == '\t')) {
        s++;
    }
    while (e > s && (in.data[e - 1] == ' ' || in.data[e - 1] == '\t')) {
        e--;
    }
    *start = s;
    *len = e - s;
}

/* Trims ASCII whitespace, lowercases and validates; returns false on
 * invalid input. `out` must hold at least BLOOM_AUTH_USERNAME_MAX + 1. */
static bool bloom_auth_normalize_username(
    bloom_str in,
    char *out,
    size_t *out_len
) {
    size_t start = 0;
    size_t len = 0;
    bloom_auth_trimmed(in, &start, &len);
    size_t end = start + len;
    size_t n = end - start;
    if (n < BLOOM_AUTH_USERNAME_MIN || n > BLOOM_AUTH_USERNAME_MAX) {
        return false;
    }
    for (size_t i = 0; i < n; i++) {
        char c = in.data[start + i];
        if (c >= 'A' && c <= 'Z') {
            c = (char)(c + 32);
        }
        bool alpha = c >= 'a' && c <= 'z';
        bool digit = c >= '0' && c <= '9';
        if (i == 0 && !alpha && !digit) {
            return false;
        }
        if (!alpha && !digit && c != '.' && c != '_' && c != '-') {
            return false;
        }
        out[i] = c;
    }
    out[n] = '\0';
    *out_len = n;
    return true;
}

/* ---- users ------------------------------------------------------------- */

bloom_enum_Result_i64_str bloom_auth_register_user(
    bloom_struct_std_db_Database database,
    bloom_str username,
    bloom_str password
) {
    char uname[BLOOM_AUTH_USERNAME_MAX + 1];
    size_t uname_len = 0;
    if (!bloom_auth_normalize_username(username, uname, &uname_len)) {
        return bloom_auth_err_int(bloom_auth_msg(
            "register_user",
            "invalid username (3..32 characters, a-z 0-9 . _ -, starting with a letter or digit)"
        ));
    }
    bloom_str hash = bloom_auth_lit("", 0);
    bloom_enum_Result_str_str hashed = bloom_auth_password_hash(password);
    if (hashed.tag == 1) {
        return bloom_auth_err_int(hashed.data.m_Err._0);
    }
    hash = hashed.data.m_Ok._0;
    bloom_str sql = bloom_auth_key(
        "INSERT INTO bloom_auth_users "
        "(username, display_name, password_hash, active, auth_version, created_at) "
        "VALUES (?1, ?2, ?3, 1, 1, ?4)"
    );
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        bloom_str_drop(&hash);
        return bloom_auth_err_int(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    size_t display_start = 0;
    size_t display_len = 0;
    bloom_auth_trimmed(username, &display_start, &display_len);
    bloom_str u = bloom_str_from_bytes(uname, uname_len);
    bloom_str d = bloom_str_from_bytes(
        username.data + display_start,
        display_len
    );
    bloom_enum_Result_bool_str b1 = bloom_stmt_bind_text(st, 1, u);
    bloom_enum_Result_bool_str b2 = b1.tag == 0
        ? bloom_stmt_bind_text(st, 2, d)
        : b1;
    bloom_enum_Result_bool_str b3 = b2.tag == 0
        ? bloom_stmt_bind_text(st, 3, hash)
        : b2;
    int64_t now = bloom_time_unix_seconds();
    bloom_enum_Result_bool_str b4 = b3.tag == 0
        ? bloom_stmt_bind_int(st, 4, now)
        : b3;
    bloom_str_drop(&u);
    bloom_str_drop(&d);
    bloom_str_drop(&hash);
    bloom_enum_Result_i64_str result;
    if (b1.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = b1.data.m_Err._0;
    } else if (b2.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = b2.data.m_Err._0;
    } else if (b3.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = b3.data.m_Err._0;
    } else if (b4.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = b4.data.m_Err._0;
    } else {
        result = bloom_stmt_execute(st);
    }
    bloom_db_stmt_release(&st.f_id);
    if (result.tag == 1) {
        if (bloom_auth_contains(result.data.m_Err._0, "UNIQUE constraint failed")) {
            bloom_str_drop(&result.data.m_Err._0);
            return bloom_auth_err_int(bloom_auth_msg(
                "register_user",
                "username is already taken"
            ));
        }
        return result;
    }
    return bloom_auth_ok_int(bloom_db_last_insert_rowid(database));
}

/* Loads id/password_hash/active/auth_version for a normalized name. */
static bloom_enum_Result_bool_str bloom_auth_lookup_credentials(
    bloom_struct_std_db_Database database,
    const char *uname,
    size_t uname_len,
    int64_t *user_id,
    bloom_str *hash,
    bool *active,
    int64_t *auth_version
) {
    bloom_str sql = bloom_auth_key(
        "SELECT id, password_hash, active, auth_version "
        "FROM bloom_auth_users WHERE username = ?1"
    );
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        return bloom_auth_err_bool(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_str u = bloom_str_from_bytes(uname, uname_len);
    bloom_enum_Result_bool_str bound = bloom_stmt_bind_text(st, 1, u);
    bloom_str_drop(&u);
    if (bound.tag == 1) {
        bloom_db_stmt_release(&st.f_id);
        return bound;
    }
    bloom_enum_Result_bool_str stepped = bloom_stmt_step(st);
    if (stepped.tag == 1) {
        bloom_db_stmt_release(&st.f_id);
        return stepped;
    }
    if (!stepped.data.m_Ok._0) {
        bloom_db_stmt_release(&st.f_id);
        return bloom_auth_ok_bool(false);
    }
    bloom_enum_Result_i64_str id = bloom_stmt_column_int(st, 0);
    bloom_enum_Result_str_str hashed = id.tag == 0
        ? bloom_stmt_column_text(st, 1)
        : (bloom_enum_Result_str_str){1, {.m_Err = {. _0 = {0, NULL, 0}}}};
    bloom_enum_Result_bool_str act = hashed.tag == 0
        ? bloom_stmt_column_bool(st, 2)
        : (bloom_enum_Result_bool_str){1, {.m_Err = {. _0 = {0, NULL, 0}}}};
    bloom_enum_Result_i64_str ver = act.tag == 0
        ? bloom_stmt_column_int(st, 3)
        : (bloom_enum_Result_i64_str){1, {.m_Err = {. _0 = {0, NULL, 0}}}};
    bloom_db_stmt_release(&st.f_id);
    if (id.tag == 1) {
        return bloom_auth_err_bool(id.data.m_Err._0);
    }
    if (hashed.tag == 1) {
        return bloom_auth_err_bool(hashed.data.m_Err._0);
    }
    if (act.tag == 1) {
        bloom_str_drop(&hashed.data.m_Ok._0);
        return bloom_auth_err_bool(act.data.m_Err._0);
    }
    if (ver.tag == 1) {
        bloom_str_drop(&hashed.data.m_Ok._0);
        return bloom_auth_err_bool(ver.data.m_Err._0);
    }
    *user_id = id.data.m_Ok._0;
    *hash = hashed.data.m_Ok._0;
    *active = act.data.m_Ok._0;
    *auth_version = ver.data.m_Ok._0;
    return bloom_auth_ok_bool(true);
}

bloom_enum_Result_i64_str bloom_auth_authenticate(
    bloom_struct_std_db_Database database,
    bloom_str username,
    bloom_str password
) {
    char uname[BLOOM_AUTH_USERNAME_MAX + 1];
    size_t uname_len = 0;
    if (!bloom_auth_normalize_username(username, uname, &uname_len)) {
        bloom_str dummy = bloom_auth_key(BLOOM_AUTH_DUMMY_HASH);
        bloom_auth_password_verify(password, dummy);
        return bloom_auth_err_int(bloom_auth_msg(
            "authenticate",
            "invalid credentials"
        ));
    }
    int64_t user_id = 0;
    int64_t version = 0;
    bloom_str hash = {0, NULL, 0};
    bool active = false;
    bloom_enum_Result_bool_str found = bloom_auth_lookup_credentials(
        database,
        uname,
        uname_len,
        &user_id,
        &hash,
        &active,
        &version
    );
    if (found.tag == 1) {
        return bloom_auth_err_int(found.data.m_Err._0);
    }
    if (!found.data.m_Ok._0) {
        bloom_str dummy = bloom_auth_key(BLOOM_AUTH_DUMMY_HASH);
        bloom_auth_password_verify(password, dummy);
        return bloom_auth_err_int(bloom_auth_msg(
            "authenticate",
            "invalid credentials"
        ));
    }
    bloom_enum_Result_bool_str verified = bloom_auth_password_verify(password, hash);
    bloom_str_drop(&hash);
    if (verified.tag == 1) {
        bloom_str_drop(&verified.data.m_Err._0);
        return bloom_auth_err_int(bloom_auth_msg(
            "authenticate",
            "invalid credentials"
        ));
    }
    if (!verified.data.m_Ok._0 || !active) {
        return bloom_auth_err_int(bloom_auth_msg(
            "authenticate",
            "invalid credentials"
        ));
    }
    return bloom_auth_ok_int(user_id);
}

static bloom_enum_Result_std_auth_UserInfo_str bloom_auth_user_row(
    bloom_struct_std_db_Statement st
) {
    bloom_enum_Result_i64_str id = bloom_stmt_column_int(st, 0);
    bloom_enum_Result_str_str username = id.tag == 0
        ? bloom_stmt_column_text(st, 1)
        : (bloom_enum_Result_str_str){1, {.m_Err = {. _0 = {0, NULL, 0}}}};
    bloom_enum_Result_str_str display = username.tag == 0
        ? bloom_stmt_column_text(st, 2)
        : (bloom_enum_Result_str_str){1, {.m_Err = {. _0 = {0, NULL, 0}}}};
    bloom_enum_Result_bool_str active = display.tag == 0
        ? bloom_stmt_column_bool(st, 3)
        : (bloom_enum_Result_bool_str){1, {.m_Err = {. _0 = {0, NULL, 0}}}};
    bloom_enum_Result_i64_str version = active.tag == 0
        ? bloom_stmt_column_int(st, 4)
        : (bloom_enum_Result_i64_str){1, {.m_Err = {. _0 = {0, NULL, 0}}}};
    if (id.tag == 1) {
        return bloom_auth_err_user(id.data.m_Err._0);
    }
    if (username.tag == 1) {
        return bloom_auth_err_user(username.data.m_Err._0);
    }
    if (display.tag == 1) {
        bloom_str_drop(&username.data.m_Ok._0);
        return bloom_auth_err_user(display.data.m_Err._0);
    }
    if (active.tag == 1) {
        bloom_str_drop(&username.data.m_Ok._0);
        bloom_str_drop(&display.data.m_Ok._0);
        return bloom_auth_err_user(active.data.m_Err._0);
    }
    if (version.tag == 1) {
        bloom_str_drop(&username.data.m_Ok._0);
        bloom_str_drop(&display.data.m_Ok._0);
        return bloom_auth_err_user(version.data.m_Err._0);
    }
    bloom_struct_std_auth_UserInfo info;
    info.f_id = id.data.m_Ok._0;
    info.f_username = username.data.m_Ok._0;
    info.f_display_name = display.data.m_Ok._0;
    info.f_active = active.data.m_Ok._0;
    info.f_auth_version = version.data.m_Ok._0;
    return bloom_auth_ok_user(info);
}

bloom_enum_Result_std_auth_UserInfo_str bloom_auth_user_get(
    bloom_struct_std_db_Database database,
    int64_t user_id
) {
    bloom_str sql = bloom_auth_key(
        "SELECT id, username, display_name, active, auth_version "
        "FROM bloom_auth_users WHERE id = ?1"
    );
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        return bloom_auth_err_user(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_enum_Result_bool_str bound = bloom_stmt_bind_int(st, 1, user_id);
    if (bound.tag == 1) {
        bloom_enum_Result_std_auth_UserInfo_str r =
            bloom_auth_err_user(bound.data.m_Err._0);
        bloom_db_stmt_release(&st.f_id);
        return r;
    }
    bloom_enum_Result_bool_str stepped = bloom_stmt_step(st);
    if (stepped.tag == 1) {
        bloom_enum_Result_std_auth_UserInfo_str r =
            bloom_auth_err_user(stepped.data.m_Err._0);
        bloom_db_stmt_release(&st.f_id);
        return r;
    }
    if (!stepped.data.m_Ok._0) {
        bloom_db_stmt_release(&st.f_id);
        return bloom_auth_err_user(bloom_auth_msg("user_get", "user not found"));
    }
    bloom_enum_Result_std_auth_UserInfo_str r = bloom_auth_user_row(st);
    bloom_db_stmt_release(&st.f_id);
    return r;
}

/* Looks up a user id by (normalized) name; Err when the user does not exist. */
bloom_enum_Result_i64_str bloom_auth_user_find(
    bloom_struct_std_db_Database database,
    bloom_str username
) {
    char uname[BLOOM_AUTH_USERNAME_MAX + 1];
    size_t uname_len = 0;
    if (!bloom_auth_normalize_username(username, uname, &uname_len)) {
        return bloom_auth_err_int(bloom_auth_msg("user_find", "user not found"));
    }
    bloom_str sql = bloom_auth_key("SELECT id FROM bloom_auth_users WHERE username = ?1");
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        return bloom_auth_err_int(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_str u = bloom_str_from_bytes(uname, uname_len);
    bloom_enum_Result_bool_str bound = bloom_stmt_bind_text(st, 1, u);
    bloom_str_drop(&u);
    if (bound.tag == 1) {
        bloom_enum_Result_i64_str r = bloom_auth_err_int(bound.data.m_Err._0);
        bloom_db_stmt_release(&st.f_id);
        return r;
    }
    bloom_enum_Result_bool_str stepped = bloom_stmt_step(st);
    if (stepped.tag == 1) {
        bloom_enum_Result_i64_str r = bloom_auth_err_int(stepped.data.m_Err._0);
        bloom_db_stmt_release(&st.f_id);
        return r;
    }
    if (!stepped.data.m_Ok._0) {
        bloom_db_stmt_release(&st.f_id);
        return bloom_auth_err_int(bloom_auth_msg("user_find", "user not found"));
    }
    bloom_enum_Result_i64_str id = bloom_stmt_column_int(st, 0);
    bloom_db_stmt_release(&st.f_id);
    return id;
}

static bloom_enum_Result_bool_str bloom_auth_set_active(
    bloom_struct_std_db_Database database,
    int64_t user_id,
    int active,
    const char *op
) {
    char sqlbuf[160];
    snprintf(
        sqlbuf,
        sizeof sqlbuf,
        "UPDATE bloom_auth_users SET active = %d, "
        "auth_version = auth_version + 1 WHERE id = ?1",
        active
    );
    bloom_str sql = bloom_auth_lit(sqlbuf, strlen(sqlbuf));
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        return bloom_auth_err_bool(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_enum_Result_bool_str bound = bloom_stmt_bind_int(st, 1, user_id);
    if (bound.tag == 1) {
        bloom_enum_Result_bool_str r = bound;
        bloom_db_stmt_release(&st.f_id);
        return r;
    }
    bloom_enum_Result_i64_str changed = bloom_stmt_execute(st);
    bloom_db_stmt_release(&st.f_id);
    if (changed.tag == 1) {
        return bloom_auth_err_bool(changed.data.m_Err._0);
    }
    if (changed.data.m_Ok._0 == 0) {
        return bloom_auth_err_bool(bloom_auth_msg(op, "user not found"));
    }
    return bloom_auth_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_auth_user_disable(
    bloom_struct_std_db_Database database,
    int64_t user_id
) {
    return bloom_auth_set_active(database, user_id, 0, "user_disable");
}

bloom_enum_Result_bool_str bloom_auth_user_enable(
    bloom_struct_std_db_Database database,
    int64_t user_id
) {
    return bloom_auth_set_active(database, user_id, 1, "user_enable");
}

bloom_enum_Result_bool_str bloom_auth_password_change(
    bloom_struct_std_db_Database database,
    int64_t user_id,
    bloom_str new_password
) {
    bloom_enum_Result_str_str hashed = bloom_auth_password_hash(new_password);
    if (hashed.tag == 1) {
        return bloom_auth_err_bool(hashed.data.m_Err._0);
    }
    bloom_str hash = hashed.data.m_Ok._0;
    bloom_str sql = bloom_auth_key(
        "UPDATE bloom_auth_users SET password_hash = ?2, "
        "auth_version = auth_version + 1 WHERE id = ?1"
    );
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        bloom_str_drop(&hash);
        return bloom_auth_err_bool(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_enum_Result_bool_str b1 = bloom_stmt_bind_int(st, 1, user_id);
    bloom_enum_Result_bool_str b2 = b1.tag == 0
        ? bloom_stmt_bind_text(st, 2, hash)
        : b1;
    bloom_str_drop(&hash);
    bloom_enum_Result_bool_str result;
    if (b1.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = b1.data.m_Err._0;
    } else if (b2.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = b2.data.m_Err._0;
    } else {
        bloom_enum_Result_i64_str changed = bloom_stmt_execute(st);
        if (changed.tag == 1) {
            result.tag = 1;
            result.data.m_Err._0 = changed.data.m_Err._0;
        } else if (changed.data.m_Ok._0 == 0) {
            result = bloom_auth_err_bool(bloom_auth_msg(
                "password_change",
                "user not found"
            ));
        } else {
            result = bloom_auth_ok_bool(true);
        }
    }
    bloom_db_stmt_release(&st.f_id);
    return result;
}

/* ---- authenticated sessions ------------------------------------------- */

static bloom_str bloom_auth_session_key_id(void) {
    return bloom_auth_key("_auth_user_id");
}

static bloom_str bloom_auth_session_key_version(void) {
    return bloom_auth_key("_auth_version");
}

static bool bloom_auth_session_set_keys(
    bloom_struct_std_web_Session session,
    int64_t user_id,
    int64_t auth_version
) {
    bloom_struct_std_json_JsonValue data = bloom_web_session_data(session);
    if (data.f_id == 0) {
        return false;
    }
    bool ok = true;
    bloom_struct_std_json_JsonValue v = bloom_json_make_int(user_id);
    bloom_enum_Result_bool_str r = bloom_json_set(
        data,
        bloom_auth_session_key_id(),
        v
    );
    if (r.tag == 1) {
        bloom_str_drop(&r.data.m_Err._0);
        bloom_json_release(&v.f_id);
        bloom_json_release(&data.f_id);
        return false;
    }
    bloom_json_release(&v.f_id);
    v = bloom_json_make_int(auth_version);
    r = bloom_json_set(data, bloom_auth_session_key_version(), v);
    if (r.tag == 1) {
        bloom_str_drop(&r.data.m_Err._0);
        ok = false;
    }
    bloom_json_release(&v.f_id);
    bloom_json_release(&data.f_id);
    return ok;
}

bloom_enum_Result_std_web_Session_str bloom_auth_login(
    bloom_struct_std_web_SessionStore store,
    bloom_struct_std_web_Session session,
    bloom_struct_std_db_Database database,
    bloom_str username,
    bloom_str password
) {
    bloom_enum_Result_std_web_Session_str r;
    bloom_enum_Result_i64_str authenticated = bloom_auth_authenticate(
        database,
        username,
        password
    );
    if (authenticated.tag == 1) {
        r.tag = 1;
        r.data.m_Err._0 = authenticated.data.m_Err._0;
        return r;
    }
    int64_t user_id = authenticated.data.m_Ok._0;
    bloom_enum_Result_std_auth_UserInfo_str user = bloom_auth_user_get(
        database,
        user_id
    );
    if (user.tag == 1) {
        r.tag = 1;
        r.data.m_Err._0 = user.data.m_Err._0;
        return r;
    }
    int64_t version = user.data.m_Ok._0.f_auth_version;
    bloom_str_drop(&user.data.m_Ok._0.f_username);
    bloom_str_drop(&user.data.m_Ok._0.f_display_name);
    bloom_enum_Result_std_web_Session_str rotated = bloom_web_session_regenerate(
        store,
        session
    );
    if (rotated.tag == 1) {
        return rotated;
    }
    if (!bloom_auth_session_set_keys(rotated.data.m_Ok._0, user_id, version)) {
        bloom_web_session_destroy(store, rotated.data.m_Ok._0);
        bloom_session_handle leftover = rotated.data.m_Ok._0.f_id;
        bloom_web_session_release(&leftover);
        r.tag = 1;
        r.data.m_Err._0 = bloom_auth_msg(
            "login",
            "session data could not be updated"
        );
        return r;
    }
    return rotated;
}

bloom_enum_Result_bool_str bloom_auth_logout(
    bloom_struct_std_web_SessionStore store,
    bloom_struct_std_web_Session session
) {
    bool destroyed = bloom_web_session_destroy(store, session);
    return bloom_auth_ok_bool(destroyed);
}

bloom_enum_Result_Option_std_auth_UserInfo_str bloom_auth_current_user(
    bloom_struct_std_db_Database database,
    bloom_struct_std_web_SessionStore store,
    bloom_struct_std_web_Session session
) {
    if (!bloom_web_session_registered(store, session)) {
        return bloom_auth_ok_opt(bloom_auth_none_user());
    }
    bloom_struct_std_json_JsonValue data = bloom_web_session_data(session);
    if (data.f_id == 0) {
        return bloom_auth_ok_opt(bloom_auth_none_user());
    }
    bloom_enum_Result_i64_str id = bloom_json_get_int(
        data,
        bloom_auth_session_key_id()
    );
    bloom_enum_Result_i64_str version = bloom_json_get_int(
        data,
        bloom_auth_session_key_version()
    );
    bloom_json_release(&data.f_id);
    if (id.tag == 1) {
        bloom_str_drop(&id.data.m_Err._0);
        if (version.tag == 0) {
            /* nothing owned */
        } else {
            bloom_str_drop(&version.data.m_Err._0);
        }
        return bloom_auth_ok_opt(bloom_auth_none_user());
    }
    if (version.tag == 1) {
        bloom_str_drop(&version.data.m_Err._0);
        return bloom_auth_ok_opt(bloom_auth_none_user());
    }
    bloom_enum_Result_std_auth_UserInfo_str user = bloom_auth_user_get(
        database,
        id.data.m_Ok._0
    );
    if (user.tag == 1) {
        bloom_str_drop(&user.data.m_Err._0);
        return bloom_auth_ok_opt(bloom_auth_none_user());
    }
    bloom_struct_std_auth_UserInfo info = user.data.m_Ok._0;
    if (!info.f_active || info.f_auth_version != version.data.m_Ok._0) {
        bloom_str_drop(&info.f_username);
        bloom_str_drop(&info.f_display_name);
        return bloom_auth_ok_opt(bloom_auth_none_user());
    }
    return bloom_auth_ok_opt(bloom_auth_some_user(info));
}

/* ---- roles ------------------------------------------------------------- */

static bool bloom_auth_normalize_role(bloom_str in, char *out, size_t *out_len) {
    if (in.len == 0 || in.len > 64) {
        return false;
    }
    for (size_t i = 0; i < in.len; i++) {
        char c = in.data[i];
        if (c >= 'A' && c <= 'Z') {
            c = (char)(c + 32);
        }
        bool alpha = c >= 'a' && c <= 'z';
        bool digit = c >= '0' && c <= '9';
        if (!alpha && !digit && c != '.' && c != '_' && c != '-') {
            return false;
        }
        out[i] = c;
    }
    out[in.len] = '\0';
    *out_len = in.len;
    return true;
}

static bloom_enum_Result_bool_str bloom_auth_role_norm(
    bloom_str role,
    char *out,
    size_t *out_len
) {
    if (!bloom_auth_normalize_role(role, out, out_len)) {
        return bloom_auth_err_bool(bloom_auth_msg(
            "role",
            "invalid role name (1..64 characters, a-z 0-9 . _ -)"
        ));
    }
    return (bloom_enum_Result_bool_str){0, {.m_Ok = {true}}};
}

bloom_enum_Result_bool_str bloom_auth_role_create(
    bloom_struct_std_db_Database database,
    bloom_str role
) {
    char name[65];
    size_t name_len = 0;
    bloom_enum_Result_bool_str valid = bloom_auth_role_norm(role, name, &name_len);
    if (valid.tag == 1) {
        return valid;
    }
    bloom_str sql = bloom_auth_key("INSERT INTO bloom_auth_roles (name) VALUES (?1)");
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        return bloom_auth_err_bool(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_str n = bloom_str_from_bytes(name, name_len);
    bloom_enum_Result_bool_str bound = bloom_stmt_bind_text(st, 1, n);
    bloom_str_drop(&n);
    bloom_enum_Result_bool_str result = bound;
    if (bound.tag == 0) {
        bloom_enum_Result_i64_str changed = bloom_stmt_execute(st);
        if (changed.tag == 1) {
            if (bloom_auth_contains(
                    changed.data.m_Err._0,
                    "UNIQUE constraint failed"
                )) {
                bloom_str_drop(&changed.data.m_Err._0);
                result = bloom_auth_ok_bool(false);
            } else {
                result.tag = 1;
                result.data.m_Err._0 = changed.data.m_Err._0;
            }
        } else {
            result = bloom_auth_ok_bool(changed.data.m_Ok._0 > 0);
        }
    }
    bloom_db_stmt_release(&st.f_id);
    return result;
}

bloom_enum_Result_bool_str bloom_auth_role_delete(
    bloom_struct_std_db_Database database,
    bloom_str role
) {
    char name[65];
    size_t name_len = 0;
    bloom_enum_Result_bool_str valid = bloom_auth_role_norm(role, name, &name_len);
    if (valid.tag == 1) {
        return valid;
    }
    bloom_enum_Result_bool_str begun = bloom_db_begin(database);
    if (begun.tag == 1) {
        return begun;
    }
    bloom_str sql = bloom_auth_key(
        "DELETE FROM bloom_auth_user_roles WHERE role = ?1"
    );
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        bloom_db_rollback(database);
        return bloom_auth_err_bool(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_str n = bloom_str_from_bytes(name, name_len);
    bloom_enum_Result_bool_str bound = bloom_stmt_bind_text(st, 1, n);
    bloom_str_drop(&n);
    bloom_enum_Result_i64_str assignments = bound.tag == 0
        ? bloom_stmt_execute(st)
        : (bloom_enum_Result_i64_str){1, {.m_Err = {. _0 = {0, NULL, 0}}}};
    bloom_db_stmt_release(&st.f_id);
    if (bound.tag == 1) {
        bloom_db_rollback(database);
        return bloom_auth_err_bool(bound.data.m_Err._0);
    }
    if (assignments.tag == 1) {
        bloom_str e = assignments.data.m_Err._0;
        bloom_db_rollback(database);
        return bloom_auth_err_bool(e);
    }
    bloom_str sql2 = bloom_auth_key("DELETE FROM bloom_auth_roles WHERE name = ?1");
    bloom_enum_Result_std_db_Statement_str prep2 = bloom_db_prepare(database, sql2);
    if (prep2.tag == 1) {
        bloom_db_rollback(database);
        return bloom_auth_err_bool(prep2.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st2 = prep2.data.m_Ok._0;
    bloom_str n2 = bloom_str_from_bytes(name, name_len);
    bloom_enum_Result_bool_str bound2 = bloom_stmt_bind_text(st2, 1, n2);
    bloom_str_drop(&n2);
    bloom_enum_Result_i64_str removed = bound2.tag == 0
        ? bloom_stmt_execute(st2)
        : (bloom_enum_Result_i64_str){1, {.m_Err = {. _0 = {0, NULL, 0}}}};
    bloom_db_stmt_release(&st2.f_id);
    if (bound2.tag == 1) {
        bloom_db_rollback(database);
        return bloom_auth_err_bool(bound2.data.m_Err._0);
    }
    if (removed.tag == 1) {
        bloom_str e = removed.data.m_Err._0;
        bloom_db_rollback(database);
        return bloom_auth_err_bool(e);
    }
    bloom_enum_Result_bool_str committed = bloom_db_commit(database);
    if (committed.tag == 1) {
        bloom_db_rollback(database);
        return committed;
    }
    return bloom_auth_ok_bool(removed.data.m_Ok._0 > 0);
}

static bloom_enum_Result_bool_str bloom_auth_scalar_bool(
    bloom_struct_std_db_Database database,
    const char *sql_text,
    int64_t user_id,
    const char *role_text,
    size_t role_len
) {
    bloom_str sql = bloom_auth_key(sql_text);
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        return bloom_auth_err_bool(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_enum_Result_bool_str b1 = bloom_stmt_bind_int(st, 1, user_id);
    bloom_str role_value = bloom_str_from_bytes(role_text, role_len);
    bloom_enum_Result_bool_str b2 = b1.tag == 0
        ? bloom_stmt_bind_text(st, 2, role_value)
        : b1;
    bloom_str_drop(&role_value);
    if (b1.tag == 1) {
        bloom_enum_Result_bool_str r = b1;
        bloom_db_stmt_release(&st.f_id);
        return r;
    }
    if (b2.tag == 1) {
        bloom_enum_Result_bool_str r = b2;
        bloom_db_stmt_release(&st.f_id);
        return r;
    }
    bloom_enum_Result_bool_str stepped = bloom_stmt_step(st);
    if (stepped.tag == 1) {
        bloom_db_stmt_release(&st.f_id);
        return stepped;
    }
    if (!stepped.data.m_Ok._0) {
        bloom_db_stmt_release(&st.f_id);
        return bloom_auth_ok_bool(false);
    }
    bloom_enum_Result_i64_str count = bloom_stmt_column_int(st, 0);
    bloom_db_stmt_release(&st.f_id);
    if (count.tag == 1) {
        return bloom_auth_err_bool(count.data.m_Err._0);
    }
    return bloom_auth_ok_bool(count.data.m_Ok._0 > 0);
}

bloom_enum_Result_bool_str bloom_auth_role_assign(
    bloom_struct_std_db_Database database,
    int64_t user_id,
    bloom_str role
) {
    char name[65];
    size_t name_len = 0;
    bloom_enum_Result_bool_str valid = bloom_auth_role_norm(role, name, &name_len);
    if (valid.tag == 1) {
        return valid;
    }
    bloom_enum_Result_std_auth_UserInfo_str user = bloom_auth_user_get(
        database,
        user_id
    );
    if (user.tag == 1) {
        bloom_str_drop(&user.data.m_Err._0);
        return bloom_auth_err_bool(bloom_auth_msg("role_assign", "unknown user"));
    }
    bloom_str_drop(&user.data.m_Ok._0.f_username);
    bloom_str_drop(&user.data.m_Ok._0.f_display_name);
    bloom_enum_Result_bool_str exists = bloom_auth_scalar_bool(
        database,
        "SELECT count(*) FROM bloom_auth_roles WHERE name = ?2 AND ?1 = ?1",
        user_id,
        name,
        name_len
    );
    if (exists.tag == 1) {
        return exists;
    }
    if (!exists.data.m_Ok._0) {
        return bloom_auth_err_bool(bloom_auth_msg("role_assign", "unknown role"));
    }
    bloom_str sql = bloom_auth_key(
        "INSERT OR IGNORE INTO bloom_auth_user_roles (user_id, role) "
        "VALUES (?1, ?2)"
    );
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        return bloom_auth_err_bool(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_enum_Result_bool_str b1 = bloom_stmt_bind_int(st, 1, user_id);
    bloom_str n = bloom_str_from_bytes(name, name_len);
    bloom_enum_Result_bool_str b2 = b1.tag == 0
        ? bloom_stmt_bind_text(st, 2, n)
        : b1;
    bloom_str_drop(&n);
    bloom_enum_Result_bool_str result;
    if (b1.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = b1.data.m_Err._0;
    } else if (b2.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = b2.data.m_Err._0;
    } else {
        bloom_enum_Result_i64_str changed = bloom_stmt_execute(st);
        if (changed.tag == 1) {
            result.tag = 1;
            result.data.m_Err._0 = changed.data.m_Err._0;
        } else {
            result = bloom_auth_ok_bool(changed.data.m_Ok._0 > 0);
        }
    }
    bloom_db_stmt_release(&st.f_id);
    return result;
}

bloom_enum_Result_bool_str bloom_auth_role_remove(
    bloom_struct_std_db_Database database,
    int64_t user_id,
    bloom_str role
) {
    char name[65];
    size_t name_len = 0;
    bloom_enum_Result_bool_str valid = bloom_auth_role_norm(role, name, &name_len);
    if (valid.tag == 1) {
        return valid;
    }
    bloom_str sql = bloom_auth_key(
        "DELETE FROM bloom_auth_user_roles WHERE user_id = ?1 AND role = ?2"
    );
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        return bloom_auth_err_bool(prep.data.m_Err._0);
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_enum_Result_bool_str b1 = bloom_stmt_bind_int(st, 1, user_id);
    bloom_str n = bloom_str_from_bytes(name, name_len);
    bloom_enum_Result_bool_str b2 = b1.tag == 0
        ? bloom_stmt_bind_text(st, 2, n)
        : b1;
    bloom_str_drop(&n);
    bloom_enum_Result_bool_str result;
    if (b1.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = b1.data.m_Err._0;
    } else if (b2.tag == 1) {
        result.tag = 1;
        result.data.m_Err._0 = b2.data.m_Err._0;
    } else {
        bloom_enum_Result_i64_str changed = bloom_stmt_execute(st);
        if (changed.tag == 1) {
            result.tag = 1;
            result.data.m_Err._0 = changed.data.m_Err._0;
        } else {
            result = bloom_auth_ok_bool(changed.data.m_Ok._0 > 0);
        }
    }
    bloom_db_stmt_release(&st.f_id);
    return result;
}

bloom_enum_Result_bool_str bloom_auth_has_role(
    bloom_struct_std_db_Database database,
    int64_t user_id,
    bloom_str role
) {
    char name[65];
    size_t name_len = 0;
    bloom_enum_Result_bool_str valid = bloom_auth_role_norm(role, name, &name_len);
    if (valid.tag == 1) {
        return bloom_auth_err_bool(bloom_auth_msg(
            "has_role",
            "invalid role name"
        ));
    }
    return bloom_auth_scalar_bool(
        database,
        "SELECT count(*) FROM bloom_auth_user_roles "
        "WHERE user_id = ?1 AND role = ?2",
        user_id,
        name,
        name_len
    );
}

bloom_enum_Result_arr_str_str bloom_auth_roles_of(
    bloom_struct_std_db_Database database,
    int64_t user_id
) {
    bloom_enum_Result_arr_str_str result;
    bloom_arr_str out = bloom_arr_str_new();
    bloom_str sql = bloom_auth_key(
        "SELECT role FROM bloom_auth_user_roles WHERE user_id = ?1 ORDER BY role"
    );
    bloom_enum_Result_std_db_Statement_str prep = bloom_db_prepare(database, sql);
    if (prep.tag == 1) {
        bloom_arr_str_drop(&out);
        result.tag = 1;
        result.data.m_Err._0 = prep.data.m_Err._0;
        return result;
    }
    bloom_struct_std_db_Statement st = prep.data.m_Ok._0;
    bloom_enum_Result_bool_str bound = bloom_stmt_bind_int(st, 1, user_id);
    if (bound.tag == 1) {
        bloom_db_stmt_release(&st.f_id);
        bloom_arr_str_drop(&out);
        result.tag = 1;
        result.data.m_Err._0 = bound.data.m_Err._0;
        return result;
    }
    while (true) {
        bloom_enum_Result_bool_str stepped = bloom_stmt_step(st);
        if (stepped.tag == 1) {
            bloom_db_stmt_release(&st.f_id);
            bloom_arr_str_drop(&out);
            result.tag = 1;
            result.data.m_Err._0 = stepped.data.m_Err._0;
            return result;
        }
        if (!stepped.data.m_Ok._0) {
            break;
        }
        bloom_enum_Result_str_str role = bloom_stmt_column_text(st, 0);
        if (role.tag == 1) {
            bloom_db_stmt_release(&st.f_id);
            bloom_arr_str_drop(&out);
            result.tag = 1;
            result.data.m_Err._0 = role.data.m_Err._0;
            return result;
        }
        bloom_arr_str_push_owned(out, role.data.m_Ok._0);
    }
    bloom_db_stmt_release(&st.f_id);
    result.tag = 0;
    result.data.m_Ok._0 = out;
    return result;
}

/* ---- bounded login rate limiter --------------------------------------- */

typedef struct {
    bloom_str key;
    int64_t window_start;
    int64_t count;
} bloom_auth_bucket;

typedef struct {
    size_t rc;
    bloom_auth_bucket *items;
    size_t count;
    size_t cap;
    size_t max;
    int64_t window_ms;
    int64_t max_attempts;
} bloom_auth_limiter;

static bloom_struct_std_auth_Limiter bloom_auth_limiter_wrap(bloom_auth_limiter *l) {
    bloom_struct_std_auth_Limiter v;
    v.f_id = (bloom_auth_limiter_handle)(intptr_t)l;
    return v;
}

static bloom_auth_limiter *bloom_auth_limiter_unwrap(
    bloom_struct_std_auth_Limiter v
) {
    return (bloom_auth_limiter *)(intptr_t)v.f_id;
}

bloom_auth_limiter_handle bloom_auth_limiter_retain(bloom_auth_limiter_handle h) {
    bloom_auth_limiter *l = (bloom_auth_limiter *)(intptr_t)h;
    if (l != NULL) {
        l->rc++;
    }
    return h;
}

void bloom_auth_limiter_release(bloom_auth_limiter_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_auth_limiter *l = (bloom_auth_limiter *)(intptr_t)*h;
    *h = 0;
    if (l->rc > 0) {
        l->rc--;
    }
    if (l->rc == 0) {
        for (size_t i = 0; i < l->count; i++) {
            bloom_str_drop(&l->items[i].key);
        }
        free(l->items);
        free(l);
        bloom_live_allocs--;
    }
}

bloom_enum_Result_std_auth_Limiter_str bloom_auth_limiter_new(
    int64_t max_entries,
    int64_t window_seconds,
    int64_t max_attempts
) {
    bloom_enum_Result_std_auth_Limiter_str r;
    if (max_entries < 1 || max_entries > BLOOM_AUTH_LIMITER_MAX_ENTRIES) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_auth_msg(
            "limiter",
            "max_entries out of range (1..65536)"
        );
        return r;
    }
    if (window_seconds < 1 || window_seconds > BLOOM_AUTH_LIMITER_MAX_WINDOW) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_auth_msg(
            "limiter",
            "window_seconds out of range (1..86400)"
        );
        return r;
    }
    if (max_attempts < 1 || max_attempts > BLOOM_AUTH_LIMITER_MAX_ATTEMPTS) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_auth_msg(
            "limiter",
            "max_attempts out of range (1..1000)"
        );
        return r;
    }
    bloom_auth_limiter *l = (bloom_auth_limiter *)calloc(
        1,
        sizeof(bloom_auth_limiter)
    );
    if (l == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    l->rc = 1;
    l->max = (size_t)max_entries;
    l->window_ms = window_seconds * 1000;
    l->max_attempts = max_attempts;
    bloom_live_allocs++;
    r.tag = 0;
    r.data.m_Ok._0 = bloom_auth_limiter_wrap(l);
    return r;
}

static void bloom_auth_limiter_purge(bloom_auth_limiter *l, int64_t now) {
    size_t i = 0;
    while (i < l->count) {
        if (now - l->items[i].window_start >= l->window_ms) {
            bloom_str_drop(&l->items[i].key);
            memmove(
                &l->items[i],
                &l->items[i + 1],
                (l->count - i - 1) * sizeof *l->items
            );
            l->count--;
        } else {
            i++;
        }
    }
}

bloom_enum_Result_bool_str bloom_auth_limiter_check(
    bloom_struct_std_auth_Limiter limiter,
    bloom_str key
) {
    bloom_auth_limiter *l = bloom_auth_limiter_unwrap(limiter);
    if (l == NULL) {
        return bloom_auth_err_bool(bloom_auth_msg("limiter_check", "invalid limiter"));
    }
    if (key.len == 0 || key.len > BLOOM_AUTH_LIMITER_KEY_MAX) {
        return bloom_auth_err_bool(bloom_auth_msg(
            "limiter_check",
            "key must be 1..256 bytes"
        ));
    }
    int64_t now = bloom_time_monotonic_millis();
    bloom_auth_limiter_purge(l, now);
    for (size_t i = 0; i < l->count; i++) {
        if (l->items[i].key.len == key.len
            && memcmp(l->items[i].key.data, key.data, key.len) == 0) {
            if (l->items[i].count >= l->max_attempts) {
                return bloom_auth_ok_bool(false);
            }
            l->items[i].count++;
            return bloom_auth_ok_bool(true);
        }
    }
    size_t index = l->count;
    if (l->count >= l->max) {
        /* Full and nothing expired: evict the entry whose window is oldest.
         * Memory stays bounded; a flood of unique keys can therefore evict
         * other counters (documented limitation of this per-process limiter). */
        index = 0;
        for (size_t i = 1; i < l->count; i++) {
            if (l->items[i].window_start < l->items[index].window_start) {
                index = i;
            }
        }
        bloom_str_drop(&l->items[index].key);
    }
    if (index == l->count) {
        if (l->count == l->cap) {
            size_t cap = l->cap ? l->cap * 2 : 16;
            if (cap > l->max) {
                cap = l->max;
            }
            bloom_auth_bucket *nb = (bloom_auth_bucket *)realloc(
                l->items,
                cap * sizeof *nb
            );
            if (nb == NULL) {
                bloom_panicf(NULL, 0, "out of memory");
            }
            l->items = nb;
            l->cap = cap;
        }
        l->count++;
    }
    l->items[index].key = bloom_str_clone(key);
    l->items[index].window_start = now;
    l->items[index].count = 1;
    return bloom_auth_ok_bool(true);
}

int64_t bloom_auth_limiter_expire(bloom_struct_std_auth_Limiter limiter) {
    bloom_auth_limiter *l = bloom_auth_limiter_unwrap(limiter);
    if (l == NULL) {
        return 0;
    }
    size_t before = l->count;
    bloom_auth_limiter_purge(l, bloom_time_monotonic_millis());
    return (int64_t)(before - l->count);
}

int64_t bloom_auth_limiter_count(bloom_struct_std_auth_Limiter limiter) {
    bloom_auth_limiter *l = bloom_auth_limiter_unwrap(limiter);
    return l == NULL ? 0 : (int64_t)l->count;
}

bool bloom_auth_limiter_reset(
    bloom_struct_std_auth_Limiter limiter,
    bloom_str key
) {
    bloom_auth_limiter *l = bloom_auth_limiter_unwrap(limiter);
    if (l == NULL) {
        return false;
    }
    for (size_t i = 0; i < l->count; i++) {
        if (l->items[i].key.len == key.len
            && memcmp(l->items[i].key.data, key.data, key.len) == 0) {
            bloom_str_drop(&l->items[i].key);
            memmove(
                &l->items[i],
                &l->items[i + 1],
                (l->count - i - 1) * sizeof *l->items
            );
            l->count--;
            return true;
        }
    }
    return false;
}
