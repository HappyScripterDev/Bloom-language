/* Bloom std::cli runtime - argument parser without callbacks.
 *
 * Definitions are stored in a Command handle; parse() returns a Parsed
 * handle or a concrete error. The library never exits the process.
 * Rules:
 *   - `--name`, `--name=value`, `--name value`, `-n`, `-n=value`
 *   - flags never take values; options require one
 *   - `--` ends option parsing; later arguments are positional
 *   - unknown options, missing values and extra positionals are errors
 *   - duplicate options: the last value wins
 *   - subcommands: the first positional must match; its own definitions
 *     are used for the remaining arguments (one level)
 *   - no implicit --help/--version; register flags and print help()/your
 *     own version text.
 * Limits: 64 definitions per command, 16 subcommands, 128 parsed entries,
 * 64 positionals, 64 KiB help text.
 */

#define BLOOM_CLI_MAX_DEFS 64
#define BLOOM_CLI_MAX_SUBS 16
#define BLOOM_CLI_MAX_ENTRIES 128
#define BLOOM_CLI_MAX_POSITIONALS 64
#define BLOOM_CLI_MAX_TEXT (1024 * 1024)
#define BLOOM_CLI_MAX_HELP (64 * 1024)

typedef struct {
    bloom_str name;
    bloom_str description;
    bloom_str default_value;
    int kind; /* 0 flag, 1 option, 2 positional */
    bool required;
} bloom_cli_def;

typedef struct bloom_cli_command {
    int rc;
    bloom_str name;
    bloom_str description;
    bloom_cli_def defs[BLOOM_CLI_MAX_DEFS];
    int def_count;
    struct bloom_cli_command *subs[BLOOM_CLI_MAX_SUBS];
    int sub_count;
} bloom_cli_command;

typedef struct {
    bloom_str name;
    bloom_str value;
    bool has_value;
    bool is_default;
} bloom_cli_entry;

typedef struct {
    int rc;
    bloom_cli_entry entries[BLOOM_CLI_MAX_ENTRIES];
    int entry_count;
    bloom_str positionals[BLOOM_CLI_MAX_POSITIONALS];
    int positional_count;
    bloom_str subcommand;
} bloom_cli_parsed;

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} bloom_cli_buf;

static void bloom_cli_buf_push(bloom_cli_buf *b, const char *p, size_t n) {
    if (n == 0 || b->len + n > BLOOM_CLI_MAX_HELP) {
        return;
    }
    if (b->len + n + 1 > b->cap) {
        size_t cap = b->cap ? b->cap : 256;
        while (cap < b->len + n + 1) {
            cap *= 2;
        }
        char *fresh = (char *)realloc(b->data, cap);
        if (fresh == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        b->data = fresh;
        b->cap = cap;
    }
    memcpy(b->data + b->len, p, n);
    b->len += n;
    b->data[b->len] = '\0';
}

static void bloom_cli_buf_free(bloom_cli_buf *b) {
    free(b->data);
    b->data = NULL;
    b->len = 0;
    b->cap = 0;
}

static bloom_str bloom_cli_msg(const char *op, const char *detail) {
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

static bool bloom_cli_text_ok(bloom_str s) {
    if (s.len > BLOOM_CLI_MAX_TEXT || memchr(s.data, 0, s.len) != NULL) {
        return false;
    }
    for (size_t i = 0; i < s.len; i++) {
        unsigned char c = (unsigned char)s.data[i];
        if (c < 0x20 || c == 0x7F) {
            return false;
        }
    }
    return true;
}

static bloom_str bloom_cli_trim_dashes(bloom_str name) {
    size_t i = 0;
    while (i < name.len && name.data[i] == '-') {
        i++;
    }
    return bloom_str_from_bytes(name.data + i, name.len - i);
}

static bloom_cli_command *bloom_cli_command_from(bloom_cli_command_handle h) {
    return (bloom_cli_command *)(intptr_t)h;
}

static bloom_cli_parsed *bloom_cli_parsed_from(bloom_cli_parsed_handle h) {
    return (bloom_cli_parsed *)(intptr_t)h;
}

bloom_cli_command_handle bloom_cli_command_retain(bloom_cli_command_handle h) {
    bloom_cli_command *cmd = bloom_cli_command_from(h);
    if (cmd != NULL) {
        cmd->rc++;
    }
    return h;
}

static void bloom_cli_command_free(bloom_cli_command *cmd) {
    bloom_str_drop(&cmd->name);
    bloom_str_drop(&cmd->description);
    for (int i = 0; i < cmd->def_count; i++) {
        bloom_str_drop(&cmd->defs[i].name);
        bloom_str_drop(&cmd->defs[i].description);
        bloom_str_drop(&cmd->defs[i].default_value);
    }
    for (int i = 0; i < cmd->sub_count; i++) {
        bloom_cli_command_handle h = (bloom_cli_command_handle)(intptr_t)cmd->subs[i];
        bloom_cli_command_release(&h);
    }
    free(cmd);
}

void bloom_cli_command_release(bloom_cli_command_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_cli_command *cmd = bloom_cli_command_from(*h);
    *h = 0;
    if (cmd == NULL) {
        return;
    }
    if (cmd->rc > 0) {
        cmd->rc--;
    }
    if (cmd->rc == 0) {
        bloom_cli_command_free(cmd);
    }
}

bloom_cli_parsed_handle bloom_cli_parsed_retain(bloom_cli_parsed_handle h) {
    bloom_cli_parsed *p = bloom_cli_parsed_from(h);
    if (p != NULL) {
        p->rc++;
    }
    return h;
}

void bloom_cli_parsed_release(bloom_cli_parsed_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_cli_parsed *p = bloom_cli_parsed_from(*h);
    *h = 0;
    if (p == NULL) {
        return;
    }
    if (p->rc > 0) {
        p->rc--;
    }
    if (p->rc != 0) {
        return;
    }
    for (int i = 0; i < p->entry_count; i++) {
        bloom_str_drop(&p->entries[i].name);
        bloom_str_drop(&p->entries[i].value);
    }
    for (int i = 0; i < p->positional_count; i++) {
        bloom_str_drop(&p->positionals[i]);
    }
    bloom_str_drop(&p->subcommand);
    free(p);
}

static void bloom_cli_parsed_free(bloom_cli_parsed *p) {
    bloom_cli_parsed_handle h = (bloom_cli_parsed_handle)(intptr_t)p;
    bloom_cli_parsed_release(&h);
}

static bloom_struct_std_cli_Command bloom_cli_wrap_command(
    bloom_cli_command *cmd
) {
    bloom_struct_std_cli_Command v;
    v.f_id = (bloom_cli_command_handle)(intptr_t)cmd;
    return v;
}

static bloom_struct_std_cli_Parsed bloom_cli_wrap_parsed(bloom_cli_parsed *p) {
    bloom_struct_std_cli_Parsed v;
    v.f_id = (bloom_cli_parsed_handle)(intptr_t)p;
    return v;
}

bloom_struct_std_cli_Command bloom_cli_new_command(
    bloom_str name,
    bloom_str description
) {
    bloom_cli_command *cmd = (bloom_cli_command *)calloc(1, sizeof *cmd);
    if (cmd == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    cmd->rc = 1;
    cmd->name = bloom_str_clone(name);
    cmd->description = bloom_str_clone(description);
    return bloom_cli_wrap_command(cmd);
}

static bloom_cli_def *bloom_cli_add_def(
    bloom_cli_command *cmd,
    bloom_str name,
    bloom_str description,
    bloom_str default_value,
    int kind,
    bool required
) {
    if (cmd->def_count >= BLOOM_CLI_MAX_DEFS) {
        return NULL;
    }
    bloom_str clean = bloom_cli_trim_dashes(name);
    if (clean.len == 0 || clean.len > 64
        || !bloom_cli_text_ok(clean)
        || !bloom_cli_text_ok(description)
        || !bloom_cli_text_ok(default_value)) {
        bloom_str_drop(&clean);
        return NULL;
    }
    for (int i = 0; i < cmd->def_count; i++) {
        if (cmd->defs[i].name.len == clean.len
            && memcmp(cmd->defs[i].name.data, clean.data, clean.len) == 0) {
            bloom_str_drop(&clean);
            return NULL;
        }
    }
    bloom_cli_def *def = &cmd->defs[cmd->def_count++];
    def->name = clean;
    def->description = bloom_str_clone(description);
    def->default_value = bloom_str_clone(default_value);
    def->kind = kind;
    def->required = required;
    return def;
}

static bloom_enum_Result_bool_str bloom_cli_ok_bool(void) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = true;
    return r;
}

static bloom_enum_Result_bool_str bloom_cli_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

bloom_enum_Result_bool_str bloom_cli_flag(
    bloom_struct_std_cli_Command handle,
    bloom_str name,
    bloom_str description
) {
    bloom_cli_command *cmd = bloom_cli_command_from(handle.f_id);
    bloom_str empty = {0, NULL, 0};
    if (cmd == NULL) {
        return bloom_cli_err_bool(bloom_cli_msg("flag", "invalid command handle"));
    }
    if (bloom_cli_add_def(cmd, name, description, empty, 0, false) == NULL) {
        return bloom_cli_err_bool(bloom_cli_msg(
            "flag",
            "invalid, duplicate or too many definitions"
        ));
    }
    return bloom_cli_ok_bool();
}

bloom_enum_Result_bool_str bloom_cli_option(
    bloom_struct_std_cli_Command handle,
    bloom_str name,
    bloom_str description,
    bloom_str default_value,
    bool required
) {
    bloom_cli_command *cmd = bloom_cli_command_from(handle.f_id);
    if (cmd == NULL) {
        return bloom_cli_err_bool(bloom_cli_msg("option", "invalid command handle"));
    }
    if (bloom_cli_add_def(cmd, name, description, default_value, 1, required) == NULL) {
        return bloom_cli_err_bool(bloom_cli_msg(
            "option",
            "invalid, duplicate or too many definitions"
        ));
    }
    return bloom_cli_ok_bool();
}

bloom_enum_Result_bool_str bloom_cli_argument(
    bloom_struct_std_cli_Command handle,
    bloom_str name,
    bloom_str description,
    bool required
) {
    bloom_cli_command *cmd = bloom_cli_command_from(handle.f_id);
    bloom_str empty = {0, NULL, 0};
    if (cmd == NULL) {
        return bloom_cli_err_bool(bloom_cli_msg(
            "argument",
            "invalid command handle"
        ));
    }
    if (bloom_cli_add_def(cmd, name, description, empty, 2, required) == NULL) {
        return bloom_cli_err_bool(bloom_cli_msg(
            "argument",
            "invalid, duplicate or too many definitions"
        ));
    }
    return bloom_cli_ok_bool();
}

bloom_enum_Result_std_cli_Command_str bloom_cli_subcommand(
    bloom_struct_std_cli_Command handle,
    bloom_str name,
    bloom_str description
) {
    bloom_enum_Result_std_cli_Command_str r;
    bloom_cli_command *cmd = bloom_cli_command_from(handle.f_id);
    bloom_str clean = bloom_cli_trim_dashes(name);
    if (cmd == NULL || clean.len == 0 || !bloom_cli_text_ok(clean)
        || !bloom_cli_text_ok(description)) {
        bloom_str_drop(&clean);
        r.tag = 1;
        r.data.m_Err._0 = bloom_cli_msg("subcommand", "invalid name or description");
        return r;
    }
    if (cmd->sub_count >= BLOOM_CLI_MAX_SUBS) {
        bloom_str_drop(&clean);
        r.tag = 1;
        r.data.m_Err._0 = bloom_cli_msg("subcommand", "too many subcommands");
        return r;
    }
    bloom_cli_command *child = (bloom_cli_command *)calloc(1, sizeof *child);
    if (child == NULL) {
        bloom_str_drop(&clean);
        bloom_panicf(NULL, 0, "out of memory");
    }
    child->rc = 2; /* one reference for the parent, one for the caller */
    child->name = clean;
    child->description = bloom_str_clone(description);
    cmd->subs[cmd->sub_count++] = child;
    r.tag = 0;
    r.data.m_Ok._0 = bloom_cli_wrap_command(child);
    return r;
}

static bloom_cli_def *bloom_cli_find_def(bloom_cli_command *cmd, bloom_str name) {
    for (int i = 0; i < cmd->def_count; i++) {
        if (cmd->defs[i].name.len == name.len
            && memcmp(cmd->defs[i].name.data, name.data, name.len) == 0) {
            return &cmd->defs[i];
        }
    }
    return NULL;
}

static bloom_cli_entry *bloom_cli_add_entry(bloom_cli_parsed *p, bloom_str name) {
    if (p->entry_count >= BLOOM_CLI_MAX_ENTRIES) {
        return NULL;
    }
    bloom_cli_entry *entry = &p->entries[p->entry_count++];
    entry->name = bloom_str_clone(name);
    entry->value.data = NULL;
    entry->value.len = 0;
    entry->value.rc = 0;
    entry->has_value = false;
    entry->is_default = false;
    return entry;
}

static bloom_cli_entry *bloom_cli_find_entry(bloom_cli_parsed *p, bloom_str name) {
    for (int i = 0; i < p->entry_count; i++) {
        if (p->entries[i].name.len == name.len
            && memcmp(p->entries[i].name.data, name.data, name.len) == 0) {
            return &p->entries[i];
        }
    }
    return NULL;
}

bloom_enum_Result_std_cli_Parsed_str bloom_cli_parse(
    bloom_struct_std_cli_Command handle,
    bloom_arr_str args
) {
    bloom_enum_Result_std_cli_Parsed_str r;
    bloom_cli_command *cmd = bloom_cli_command_from(handle.f_id);
    if (cmd == NULL) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_cli_msg("parse", "invalid command handle");
        return r;
    }
    bloom_cli_parsed *p = (bloom_cli_parsed *)calloc(1, sizeof *p);
    if (p == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    p->rc = 1;
    bool end_options = false;
    size_t start = 0;
    /* A subcommand is recognized as the first positional argument. */
    if (args->len > 0 && args->data[0].len > 0 && args->data[0].data[0] != '-') {
        bloom_str first = args->data[0];
        for (int i = 0; i < cmd->sub_count; i++) {
            if (cmd->subs[i]->name.len == first.len
                && memcmp(cmd->subs[i]->name.data, first.data, first.len) == 0) {
                p->subcommand = bloom_str_clone(first);
                cmd = cmd->subs[i];
                start = 1;
                break;
            }
        }
    }
    for (size_t i = start; i < args->len; i++) {
        bloom_str arg = args->data[i];
        if (!end_options && arg.len == 2 && memcmp(arg.data, "--", 2) == 0) {
            end_options = true;
            continue;
        }
        if (!end_options && arg.len > 2 && arg.data[0] == '-'
            && arg.data[1] == '-') {
            bloom_str body = bloom_str_from_bytes(arg.data + 2, arg.len - 2);
            bloom_str name = bloom_str_clone(body);
            bloom_str inline_value = {0, NULL, 0};
            bool has_inline = false;
            for (size_t k = 0; k < body.len; k++) {
                if (body.data[k] == '=') {
                    bloom_str_drop(&name);
                    name = bloom_str_from_bytes(body.data, k);
                    inline_value = bloom_str_from_bytes(
                        body.data + k + 1,
                        body.len - k - 1
                    );
                    has_inline = true;
                    break;
                }
            }
            bloom_cli_def *def = bloom_cli_find_def(cmd, name);
            if (def == NULL || def->kind == 2) {
                bloom_str_drop(&body);
                if (has_inline) bloom_str_drop(&inline_value);
                bloom_str_drop(&name);
                r.tag = 1;
                r.data.m_Err._0 = bloom_cli_msg("parse", "unknown option");
                bloom_cli_parsed_free(p);
                return r;
            }
            if (def->kind == 0) {
                if (has_inline) {
                    bloom_str_drop(&body);
                    bloom_str_drop(&inline_value);
                    bloom_str_drop(&name);
                    r.tag = 1;
                    r.data.m_Err._0 = bloom_cli_msg("parse", "flag does not take a value");
                    bloom_cli_parsed_free(p);
                    return r;
                }
                bloom_cli_entry *entry = bloom_cli_add_entry(p, def->name);
                if (entry == NULL) {
                    bloom_str_drop(&body);
                    bloom_str_drop(&name);
                    r.tag = 1;
                    r.data.m_Err._0 = bloom_cli_msg("parse", "too many arguments");
                    bloom_cli_parsed_free(p);
                    return r;
                }
            } else {
                bloom_str value;
                if (has_inline) {
                    value = inline_value;
                } else if (i + 1 < args->len) {
                    value = bloom_str_clone(args->data[++i]);
                } else {
                    bloom_str_drop(&body);
                    bloom_str_drop(&name);
                    r.tag = 1;
                    r.data.m_Err._0 = bloom_cli_msg("parse", "option requires a value");
                    bloom_cli_parsed_free(p);
                    return r;
                }
                bloom_cli_entry *entry = bloom_cli_find_entry(p, def->name);
                if (entry == NULL) {
                    entry = bloom_cli_add_entry(p, def->name);
                }
                if (entry == NULL) {
                    bloom_str_drop(&body);
                    bloom_str_drop(&name);
                    bloom_str_drop(&value);
                    r.tag = 1;
                    r.data.m_Err._0 = bloom_cli_msg("parse", "too many arguments");
                    bloom_cli_parsed_free(p);
                    return r;
                }
                bloom_str_drop(&entry->value);
                entry->value = value;
                entry->has_value = true;
            }
            bloom_str_drop(&body);
            if (name.data != body.data) {
                bloom_str_drop(&name);
            }
            continue;
        }
        if (!end_options && arg.len > 1 && arg.data[0] == '-') {
            bloom_str body = bloom_str_from_bytes(arg.data + 1, arg.len - 1);
            bloom_str name = bloom_str_clone(body);
            bloom_str inline_value = {0, NULL, 0};
            bool has_inline = false;
            for (size_t k = 0; k < body.len; k++) {
                if (body.data[k] == '=') {
                    bloom_str_drop(&name);
                    name = bloom_str_from_bytes(body.data, k);
                    inline_value = bloom_str_from_bytes(
                        body.data + k + 1,
                        body.len - k - 1
                    );
                    has_inline = true;
                    break;
                }
            }
            bloom_cli_def *def = bloom_cli_find_def(cmd, name);
            if (def != NULL && def->kind == 0 && !has_inline) {
                bloom_cli_entry *entry = bloom_cli_add_entry(p, def->name);
                if (entry == NULL) {
                    bloom_str_drop(&body);
                    bloom_str_drop(&name);
                    r.tag = 1;
                    r.data.m_Err._0 = bloom_cli_msg("parse", "too many arguments");
                    bloom_cli_parsed_free(p);
                    return r;
                }
            } else if (def != NULL && def->kind == 1) {
                bloom_str value;
                if (has_inline) {
                    value = inline_value;
                } else if (i + 1 < args->len) {
                    value = bloom_str_clone(args->data[++i]);
                } else {
                    bloom_str_drop(&body);
                    bloom_str_drop(&name);
                    r.tag = 1;
                    r.data.m_Err._0 = bloom_cli_msg("parse", "option requires a value");
                    bloom_cli_parsed_free(p);
                    return r;
                }
                bloom_cli_entry *entry = bloom_cli_find_entry(p, def->name);
                if (entry == NULL) {
                    entry = bloom_cli_add_entry(p, def->name);
                }
                if (entry == NULL) {
                    bloom_str_drop(&body);
                    bloom_str_drop(&name);
                    bloom_str_drop(&value);
                    r.tag = 1;
                    r.data.m_Err._0 = bloom_cli_msg("parse", "too many arguments");
                    bloom_cli_parsed_free(p);
                    return r;
                }
                bloom_str_drop(&entry->value);
                entry->value = value;
                entry->has_value = true;
            } else {
                bool all_flags = !has_inline && body.len > 1;
                for (size_t k = 0; all_flags && k < body.len; k++) {
                    bloom_str ch = bloom_str_from_bytes(body.data + k, 1);
                    bloom_cli_def *flag = bloom_cli_find_def(cmd, ch);
                    bloom_str_drop(&ch);
                    if (flag == NULL || flag->kind != 0) {
                        all_flags = false;
                    }
                }
                if (all_flags) {
                    for (size_t k = 0; k < body.len; k++) {
                        bloom_str ch = bloom_str_from_bytes(body.data + k, 1);
                        bloom_cli_def *flag = bloom_cli_find_def(cmd, ch);
                        bloom_str_drop(&ch);
                        if (bloom_cli_add_entry(p, flag->name) == NULL) {
                            all_flags = false;
                            break;
                        }
                    }
                }
                if (!all_flags) {
                    bloom_str_drop(&body);
                    bloom_str_drop(&name);
                    if (has_inline) bloom_str_drop(&inline_value);
                    r.tag = 1;
                    r.data.m_Err._0 = bloom_cli_msg("parse", "unknown option");
                    bloom_cli_parsed_free(p);
                    return r;
                }
            }
            bloom_str_drop(&body);
            if (name.data != body.data) {
                bloom_str_drop(&name);
            }
            continue;
        }
        if (p->positional_count >= BLOOM_CLI_MAX_POSITIONALS) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_cli_msg("parse", "too many positional arguments");
            bloom_cli_parsed_free(p);
            return r;
        }
        p->positionals[p->positional_count++] = bloom_str_clone(arg);
    }
    /* Required options and defaults. */
    for (int i = 0; i < cmd->def_count; i++) {
        bloom_cli_def *def = &cmd->defs[i];
        if (def->kind != 1) {
            continue;
        }
        bloom_cli_entry *entry = bloom_cli_find_entry(p, def->name);
        if (entry == NULL && def->default_value.len > 0) {
            entry = bloom_cli_add_entry(p, def->name);
            if (entry != NULL) {
                entry->value = bloom_str_clone(def->default_value);
                entry->has_value = true;
                entry->is_default = true;
            }
        }
        if (entry == NULL && def->required) {
            r.tag = 1;
            r.data.m_Err._0 = bloom_cli_msg("parse", "required option is missing");
            bloom_cli_parsed_free(p);
            return r;
        }
    }
    int positional_def = 0;
    for (int i = 0; i < cmd->def_count; i++) {
        if (cmd->defs[i].kind == 2) {
            if (positional_def >= p->positional_count && cmd->defs[i].required) {
                r.tag = 1;
                r.data.m_Err._0 = bloom_cli_msg("parse", "required argument is missing");
                bloom_cli_parsed_free(p);
                return r;
            }
            positional_def++;
        }
    }
    if ((int)p->positional_count > positional_def) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_cli_msg("parse", "too many positional arguments");
        bloom_cli_parsed_free(p);
        return r;
    }
    r.tag = 0;
    r.data.m_Ok._0 = bloom_cli_wrap_parsed(p);
    return r;
}

static bloom_cli_def *bloom_cli_def_by_name(bloom_cli_command *cmd, bloom_str name) {
    bloom_str clean = bloom_cli_trim_dashes(name);
    bloom_cli_def *def = bloom_cli_find_def(cmd, clean);
    bloom_str_drop(&clean);
    return def;
}

bool bloom_cli_has(
    bloom_struct_std_cli_Parsed parsed,
    bloom_str name
) {
    bloom_cli_parsed *p = bloom_cli_parsed_from(parsed.f_id);
    if (p == NULL) {
        return false;
    }
    bloom_str clean = bloom_cli_trim_dashes(name);
    bloom_cli_entry *entry = bloom_cli_find_entry(p, clean);
    bloom_str_drop(&clean);
    return entry != NULL && !entry->is_default;
}

bloom_enum_Option_str bloom_cli_value(
    bloom_struct_std_cli_Parsed parsed,
    bloom_str name
) {
    bloom_enum_Option_str r;
    bloom_cli_parsed *p = bloom_cli_parsed_from(parsed.f_id);
    if (p == NULL) {
        r.tag = 1;
        return r;
    }
    bloom_str clean = bloom_cli_trim_dashes(name);
    bloom_cli_entry *entry = bloom_cli_find_entry(p, clean);
    if (entry != NULL && entry->has_value) {
        r.tag = 0;
        r.data.m_Some._0 = bloom_str_clone(entry->value);
        bloom_str_drop(&clean);
        return r;
    }
    bloom_str_drop(&clean);
    r.tag = 1;
    return r;
}

bloom_enum_Option_str bloom_cli_positional(
    bloom_struct_std_cli_Parsed parsed,
    int64_t index
) {
    bloom_enum_Option_str r;
    bloom_cli_parsed *p = bloom_cli_parsed_from(parsed.f_id);
    if (p == NULL || index < 0 || index >= (int64_t)p->positional_count) {
        r.tag = 1;
        return r;
    }
    r.tag = 0;
    r.data.m_Some._0 = bloom_str_clone(p->positionals[index]);
    return r;
}

bloom_enum_Option_str bloom_cli_subcommand_name(
    bloom_struct_std_cli_Parsed parsed
) {
    bloom_enum_Option_str r;
    bloom_cli_parsed *p = bloom_cli_parsed_from(parsed.f_id);
    if (p == NULL || p->subcommand.len == 0) {
        r.tag = 1;
        return r;
    }
    r.tag = 0;
    r.data.m_Some._0 = bloom_str_clone(p->subcommand);
    return r;
}

static void bloom_cli_help_push(bloom_cli_buf *b, const char *text) {
    bloom_cli_buf_push(b, text, strlen(text));
}

bloom_str bloom_cli_help(bloom_struct_std_cli_Command handle) {
    bloom_cli_command *cmd = bloom_cli_command_from(handle.f_id);
    if (cmd == NULL) {
        return bloom_str_from_bytes("", 0);
    }
    bloom_cli_buf b;
    b.data = NULL;
    b.len = 0;
    b.cap = 0;
    bloom_cli_help_push(&b, "Usage: ");
    bloom_cli_help_push(&b, cmd->name.data ? cmd->name.data : "");
    if (cmd->def_count > 0) {
        bloom_cli_help_push(&b, " [options]");
    }
    bloom_cli_help_push(&b, "\n");
    if (cmd->description.len > 0) {
        bloom_cli_help_push(&b, cmd->description.data);
        bloom_cli_help_push(&b, "\n");
    }
    for (int i = 0; i < cmd->def_count; i++) {
        bloom_cli_def *def = &cmd->defs[i];
        bloom_cli_help_push(&b, "  ");
        if (def->kind == 2) {
            bloom_cli_help_push(&b, def->name.data ? def->name.data : "");
        } else {
            bloom_cli_help_push(&b, "--");
            bloom_cli_help_push(&b, def->name.data ? def->name.data : "");
            if (def->kind == 1) {
                bloom_cli_help_push(&b, " <value>");
            }
        }
        if (def->description.len > 0) {
            bloom_cli_help_push(&b, "  ");
            bloom_cli_help_push(&b, def->description.data);
        }
        if (def->required) {
            bloom_cli_help_push(&b, " (required)");
        }
        bloom_cli_help_push(&b, "\n");
        if (b.len > BLOOM_CLI_MAX_HELP) {
            break;
        }
    }
    if (cmd->sub_count > 0) {
        bloom_cli_help_push(&b, "Subcommands:\n");
        for (int i = 0; i < cmd->sub_count; i++) {
            bloom_cli_help_push(&b, "  ");
            bloom_cli_help_push(&b, cmd->subs[i]->name.data ? cmd->subs[i]->name.data : "");
            if (cmd->subs[i]->description.len > 0) {
                bloom_cli_help_push(&b, "  ");
                bloom_cli_help_push(&b, cmd->subs[i]->description.data);
            }
            bloom_cli_help_push(&b, "\n");
        }
    }
    bloom_str out = bloom_str_from_bytes(
        b.data ? b.data : "",
        b.len > BLOOM_CLI_MAX_HELP ? BLOOM_CLI_MAX_HELP : b.len
    );
    bloom_cli_buf_free(&b);
    return out;
}
