/* Bloom std::io runtime - explicit text I/O and flushing.
 *
 * Text is UTF-8 at the Bloom boundary. Bytes remain available through
 * std::fs::Bytes for files; this module keeps console I/O predictable and
 * reports errors instead of silently truncating. Limits: 8 MiB for
 * read_stdin by default, 64 MiB hard cap, 1 MiB per read_line.
 */

#define BLOOM_IO_MAX_LINE (1024 * 1024)
#define BLOOM_IO_MAX_READ (64 * 1024 * 1024)

static bloom_str bloom_io_msg(const char *op, const char *detail) {
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

static bloom_enum_Result_bool_str bloom_io_ok_bool(void) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = true;
    return r;
}

static bloom_enum_Result_bool_str bloom_io_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

bloom_enum_Result_bool_str bloom_io_stdout_write(bloom_str text) {
    if (text.len > 0 && fwrite(text.data, 1, text.len, stdout) != text.len) {
        return bloom_io_err_bool(bloom_io_msg("stdout_write", "write failed"));
    }
    return bloom_io_ok_bool();
}

bloom_enum_Result_bool_str bloom_io_stderr_write(bloom_str text) {
    if (text.len > 0 && fwrite(text.data, 1, text.len, stderr) != text.len) {
        return bloom_io_err_bool(bloom_io_msg("stderr_write", "write failed"));
    }
    return bloom_io_ok_bool();
}

bloom_enum_Result_bool_str bloom_io_stdout_flush(void) {
    if (fflush(stdout) != 0) {
        return bloom_io_err_bool(bloom_io_msg("stdout_flush", "flush failed"));
    }
    return bloom_io_ok_bool();
}

bloom_enum_Result_bool_str bloom_io_stderr_flush(void) {
    if (fflush(stderr) != 0) {
        return bloom_io_err_bool(bloom_io_msg("stderr_flush", "flush failed"));
    }
    return bloom_io_ok_bool();
}

static bool bloom_io_utf8_ok(bloom_str s) {
    size_t i = 0;
    while (i < s.len) {
        unsigned char c = (unsigned char)s.data[i];
        size_t need = 0;
        if (c < 0x80) {
            i++;
            continue;
        } else if ((c & 0xE0) == 0xC0) {
            need = 1;
        } else if ((c & 0xF0) == 0xE0) {
            need = 2;
        } else if ((c & 0xF8) == 0xF0) {
            need = 3;
        } else {
            return false;
        }
        if (i + need >= s.len) {
            return false;
        }
        for (size_t k = 1; k <= need; k++) {
            if ((s.data[i + k] & 0xC0) != 0x80) {
                return false;
            }
        }
        i += need + 1;
    }
    return true;
}
bloom_enum_Result_Option_str_str bloom_io_read_line(void) {
    bloom_enum_Result_Option_str_str r;
    char *buf = (char *)malloc(BLOOM_IO_MAX_LINE + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t len = 0;
    bool eof = false;
    for (;;) {
        int c = fgetc(stdin);
        if (c == EOF) {
            eof = feof(stdin) != 0;
            break;
        }
        if (c == '\n') {
            break;
        }
        if (len >= BLOOM_IO_MAX_LINE) {
            free(buf);
            r.tag = 1;
            r.data.m_Err._0 = bloom_io_msg("read_line", "line exceeds 1048576 bytes");
            return r;
        }
        buf[len++] = (char)c;
    }
    if (eof && len == 0) {
        free(buf);
        r.tag = 0;
        r.data.m_Ok._0.tag = 1;
        return r;
    }
    if (len > 0 && buf[len - 1] == '\r') {
        len--;
    }
    bloom_str line = bloom_str_from_bytes(buf, len);
    free(buf);
    if (!bloom_io_utf8_ok(line)) {
        bloom_str_drop(&line);
        r.tag = 1;
        r.data.m_Err._0 = bloom_io_msg("read_line", "input is not valid UTF-8");
        return r;
    }
    r.tag = 0;
    r.data.m_Ok._0.tag = 0;
    r.data.m_Ok._0.data.m_Some._0 = line;
    return r;
}

bloom_enum_Result_str_str bloom_io_read_stdin(int64_t max_bytes) {
    bloom_enum_Result_str_str r;
    if (max_bytes < 0 || max_bytes > BLOOM_IO_MAX_READ) {
        r.tag = 1;
        r.data.m_Err._0 = bloom_io_msg(
            "read_stdin",
            "max_bytes must be between 0 and 67108864"
        );
        return r;
    }
    size_t limit = max_bytes == 0 ? 8 * 1024 * 1024 : (size_t)max_bytes;
    char *buf = (char *)malloc(limit + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t len = fread(buf, 1, limit + 1, stdin);
    if (len > limit) {
        free(buf);
        r.tag = 1;
        r.data.m_Err._0 = bloom_io_msg("read_stdin", "input exceeds the limit");
        return r;
    }
    bloom_str text = bloom_str_from_bytes(buf, len);
    free(buf);
    if (!bloom_io_utf8_ok(text)) {
        bloom_str_drop(&text);
        r.tag = 1;
        r.data.m_Err._0 = bloom_io_msg("read_stdin", "input is not valid UTF-8");
        return r;
    }
    r.tag = 0;
    r.data.m_Ok._0 = text;
    return r;
}
