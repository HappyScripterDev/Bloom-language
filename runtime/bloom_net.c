/* Bloom std::net runtime — blocking TCP with an internal handle registry.
 *
 * Only emitted when the program loads `std::net` (or `std::http`).
 *
 * Bloom-visible handles are runtime IDs, never OS descriptors: the handle
 * packs a slot index and a generation counter, so closed handles are
 * rejected (including double close and stale handles after slot reuse).
 * Sockets must be closed with `tcp_close`; there is no automatic cleanup.
 *
 * Timeouts: `tcp_set_timeout` controls read/write operations (0 disables
 * the timeout). It does NOT cover DNS resolution or `tcp_connect`.
 */

#include <errno.h>
#ifndef _WIN32
#include <sys/socket.h>
#include <netinet/in.h>
#include <netdb.h>
#include <unistd.h>
#include <sys/time.h>
#include <arpa/inet.h>
#include <fcntl.h>
#include <poll.h>
#endif

#define BLOOM_NET_MAX_READ 1048576
#define BLOOM_NET_MAX_HANDLES 65536
#define BLOOM_NET_INDEX_BITS 20
#define BLOOM_NET_INDEX_MASK ((int64_t)((1 << BLOOM_NET_INDEX_BITS) - 1))

#define BLOOM_NET_KIND_FREE 0
#define BLOOM_NET_KIND_LISTENER 1
#define BLOOM_NET_KIND_CONNECTION 2
#define BLOOM_NET_KIND_TLS 3

#ifdef _WIN32
typedef SOCKET bloom_net_sock_t;
#define BLOOM_NET_INVALID_SOCK INVALID_SOCKET
#else
typedef int bloom_net_sock_t;
#define BLOOM_NET_INVALID_SOCK (-1)
#endif

typedef struct {
    int kind;
    uint32_t gen;
    bloom_net_sock_t sock;
    /* TLS session state (bloom_tls.c); NULL for plain TCP handles. */
    void *tls;
} bloom_net_entry;

static bloom_net_entry bloom_net_table[BLOOM_NET_MAX_HANDLES];
static uint32_t bloom_net_next_gen = 1;
static bool bloom_net_wsa_ready = false;

static bool bloom_net_init(void) {
#ifdef _WIN32
    if (!bloom_net_wsa_ready) {
        WSADATA data;
        if (WSAStartup(MAKEWORD(2, 2), &data) != 0) {
            return false;
        }
        bloom_net_wsa_ready = true;
    }
#endif
    return true;
}

static void bloom_net_close_sock(bloom_net_sock_t s) {
#ifdef _WIN32
    closesocket(s);
#else
    close(s);
#endif
}

static int bloom_net_recv(bloom_net_sock_t s, char *buf, int len) {
#ifdef _WIN32
    return recv(s, buf, len, 0);
#else
    return (int)read(s, buf, (size_t)len);
#endif
}

static int bloom_net_send(bloom_net_sock_t s, const char *buf, int len) {
#ifdef _WIN32
    return send(s, buf, len, 0);
#else
    return (int)write(s, buf, (size_t)len);
#endif
}

static bool bloom_net_last_was_timeout(void) {
#ifdef _WIN32
    int e = WSAGetLastError();
    return e == WSAETIMEDOUT || e == WSAEWOULDBLOCK;
#else
    return errno == EAGAIN || errno == EWOULDBLOCK;
#endif
}

static bloom_str bloom_net_msg(const char *op, const char *detail) {
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

static bloom_str bloom_net_msg2(const char *op, const char *a, const char *b) {
    size_t opn = strlen(op);
    size_t an = strlen(a);
    size_t bn = strlen(b);
    char *buf = (char *)malloc(opn + 2 + an + bn + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t at = 0;
    memcpy(buf + at, op, opn);
    at += opn;
    buf[at++] = ':';
    buf[at++] = ' ';
    memcpy(buf + at, a, an);
    at += an;
    memcpy(buf + at, b, bn);
    at += bn;
    bloom_str s = bloom_str_from_bytes(buf, at);
    free(buf);
    return s;
}

static bloom_enum_Result_i64_str bloom_net_ok(int64_t v) {
    bloom_enum_Result_i64_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_i64_str bloom_net_err(bloom_str m) {
    bloom_enum_Result_i64_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_bool_str bloom_net_ok_bool(bool v) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_bool_str bloom_net_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

/* Registers a socket and returns its Bloom handle, or -1 when the table is
 * full. */
static int64_t bloom_net_register(int kind, bloom_net_sock_t sock) {
    for (size_t i = 0; i < BLOOM_NET_MAX_HANDLES; i++) {
        if (bloom_net_table[i].kind == BLOOM_NET_KIND_FREE) {
            uint32_t gen = bloom_net_next_gen++;
            if (bloom_net_next_gen == 0) {
                bloom_net_next_gen = 1;
            }
            bloom_net_table[i].kind = kind;
            bloom_net_table[i].gen = gen;
            bloom_net_table[i].sock = sock;
            bloom_net_table[i].tls = NULL;
            bloom_live_sockets++;
            return ((int64_t)gen << BLOOM_NET_INDEX_BITS) | (int64_t)(i + 1);
        }
    }
    return -1;
}

/* Resolves a handle. Returns NULL when the handle is unknown, closed, of
 * the wrong kind, or stale. */
static bloom_net_entry *bloom_net_lookup(int64_t handle, int kind) {
    if (handle <= 0) {
        return NULL;
    }
    int64_t index = (handle & BLOOM_NET_INDEX_MASK) - 1;
    uint32_t gen = (uint32_t)((uint64_t)handle >> BLOOM_NET_INDEX_BITS);
    if (index < 0 || index >= BLOOM_NET_MAX_HANDLES) {
        return NULL;
    }
    bloom_net_entry *e = &bloom_net_table[index];
    if (e->kind != kind || e->gen != gen) {
        return NULL;
    }
    return e;
}

static bloom_str bloom_net_handle_err(const char *op, int64_t handle) {
    char buf[96];
    snprintf(buf, sizeof buf, "unknown, closed or wrong-kind handle (%" PRId64 ")", handle);
    return bloom_net_msg(op, buf);
}

/* ---- DNS and connections ---------------------------------------------- */

static bool bloom_net_host_to_buf(bloom_str host, char *buf, size_t bufsize) {
    if (host.len == 0 || host.len >= bufsize) {
        return false;
    }
    memcpy(buf, host.data, host.len);
    buf[host.len] = '\0';
    return true;
}

/* Connects with a bounded timeout using a non-blocking connect + select. */
static bool bloom_net_connect_with_timeout(
    bloom_net_sock_t sock,
    struct addrinfo *ai,
    int64_t timeout_ms
) {
#ifdef _WIN32
    u_long nonblocking = 1;
    ioctlsocket(sock, FIONBIO, &nonblocking);
    int rc = connect(sock, ai->ai_addr, (int)ai->ai_addrlen);
    bool ok = false;
    if (rc == 0) {
        ok = true;
    } else if (WSAGetLastError() == WSAEWOULDBLOCK) {
        fd_set wr;
        fd_set ex;
        FD_ZERO(&wr);
        FD_SET(sock, &wr);
        FD_ZERO(&ex);
        FD_SET(sock, &ex);
        struct timeval tv;
        tv.tv_sec = (long)(timeout_ms / 1000);
        tv.tv_usec = (long)((timeout_ms % 1000) * 1000);
        int sel = select(0, NULL, &wr, &ex, &tv);
        if (sel > 0 && FD_ISSET(sock, &wr)) {
            int err = 0;
            int len = (int)sizeof err;
            getsockopt(sock, SOL_SOCKET, SO_ERROR, (char *)&err, &len);
            ok = (err == 0);
        }
    }
    nonblocking = 0;
    ioctlsocket(sock, FIONBIO, &nonblocking);
    return ok;
#else
    int flags = fcntl(sock, F_GETFL, 0);
    fcntl(sock, F_SETFL, flags | O_NONBLOCK);
    int rc = connect(sock, ai->ai_addr, (socklen_t)ai->ai_addrlen);
    bool ok = false;
    if (rc == 0) {
        ok = true;
    } else if (errno == EINPROGRESS) {
        struct pollfd pfd;
        pfd.fd = sock;
        pfd.events = POLLOUT;
        int sel = poll(&pfd, 1, (int)timeout_ms);
        if (sel > 0) {
            int err = 0;
            socklen_t len = sizeof err;
            getsockopt(sock, SOL_SOCKET, SO_ERROR, &err, &len);
            ok = (err == 0);
        }
    }
    fcntl(sock, F_SETFL, flags);
    return ok;
#endif
}

/* Resolves and connects a TCP socket. `timeout_ms` > 0 applies to the
 * connect attempt (via a non-blocking connect); 0 means blocking.
 * Returns 0 on success, 1 when the host name could not be resolved and
 * 2 when the connection attempt failed. */
static int bloom_net_connect_raw(
    const char *hostbuf,
    int64_t port,
    int64_t timeout_ms,
    bloom_net_sock_t *out
) {
    char portbuf[16];
    snprintf(portbuf, sizeof portbuf, "%" PRId64, port);
    struct addrinfo hints;
    struct addrinfo *res = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_protocol = IPPROTO_TCP;
    int rc = getaddrinfo(hostbuf, portbuf, &hints, &res);
    if (rc != 0 || res == NULL) {
        return 1;
    }
    bloom_net_sock_t sock = BLOOM_NET_INVALID_SOCK;
    for (struct addrinfo *ai = res; ai != NULL; ai = ai->ai_next) {
        sock = socket(ai->ai_family, ai->ai_socktype, ai->ai_protocol);
        if (sock == BLOOM_NET_INVALID_SOCK) {
            continue;
        }
        if (timeout_ms <= 0) {
            if (connect(sock, ai->ai_addr, (int)ai->ai_addrlen) == 0) {
                break;
            }
        } else if (bloom_net_connect_with_timeout(sock, ai, timeout_ms)) {
            break;
        }
        bloom_net_close_sock(sock);
        sock = BLOOM_NET_INVALID_SOCK;
    }
    freeaddrinfo(res);
    if (sock == BLOOM_NET_INVALID_SOCK) {
        return 2;
    }
    *out = sock;
    return 0;
}

/* Applies a timeout to socket send/receive operations (0 clears it). */
static bool bloom_net_set_sock_timeout(bloom_net_sock_t sock, int64_t milliseconds) {
#ifdef _WIN32
    DWORD ms = milliseconds > (int64_t)0x7FFFFFFF ? 0x7FFFFFFFu : (DWORD)milliseconds;
    return setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, (const char *)&ms, sizeof ms) == 0
        && setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, (const char *)&ms, sizeof ms) == 0;
#else
    struct timeval tv;
    tv.tv_sec = (time_t)(milliseconds / 1000);
    tv.tv_usec = (suseconds_t)((milliseconds % 1000) * 1000);
    return setsockopt(sock, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv) == 0
        && setsockopt(sock, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof tv) == 0;
#endif
}

static bloom_enum_Result_i64_str bloom_net_connect_impl(bloom_str host, int64_t port) {
    if (!bloom_net_init()) {
        return bloom_net_err(bloom_net_msg("tcp_connect", "Winsock initialization failed"));
    }
    if (port < 1 || port > 65535) {
        return bloom_net_err(bloom_net_msg("tcp_connect", "port out of range (1..65535)"));
    }
    char hostbuf[512];
    if (!bloom_net_host_to_buf(host, hostbuf, sizeof hostbuf)) {
        return bloom_net_err(bloom_net_msg("tcp_connect", "empty or too long host name"));
    }
    bloom_net_sock_t sock = BLOOM_NET_INVALID_SOCK;
    int cr = bloom_net_connect_raw(hostbuf, port, 0, &sock);
    if (cr == 1) {
        return bloom_net_err(bloom_net_msg2("tcp_connect", "cannot resolve host `", hostbuf));
    }
    if (cr != 0) {
        return bloom_net_err(bloom_net_msg2("tcp_connect", "connection failed to `", hostbuf));
    }
    int64_t handle = bloom_net_register(BLOOM_NET_KIND_CONNECTION, sock);
    if (handle < 0) {
        bloom_net_close_sock(sock);
        return bloom_net_err(bloom_net_msg("tcp_connect", "no free handle slots"));
    }
    return bloom_net_ok(handle);
}

bloom_enum_Result_i64_str bloom_net_tcp_connect(bloom_str host, int64_t port) {
    return bloom_net_connect_impl(host, port);
}

static bloom_enum_Result_i64_str bloom_net_listen_impl(
    bloom_str host,
    int64_t port,
    int64_t backlog
) {
    if (!bloom_net_init()) {
        return bloom_net_err(bloom_net_msg("tcp_listen", "Winsock initialization failed"));
    }
    if (port < 0 || port > 65535) {
        return bloom_net_err(bloom_net_msg("tcp_listen", "port out of range (0..65535)"));
    }
    if (backlog < 1) {
        return bloom_net_err(bloom_net_msg("tcp_listen", "backlog must be at least 1"));
    }
    if (backlog > 4096) {
        backlog = 4096;
    }
    char hostbuf[512];
    const char *hostptr = NULL;
    if (host.len > 0) {
        if (!bloom_net_host_to_buf(host, hostbuf, sizeof hostbuf)) {
            return bloom_net_err(bloom_net_msg("tcp_listen", "too long host name"));
        }
        hostptr = hostbuf;
    }
    char portbuf[16];
    snprintf(portbuf, sizeof portbuf, "%" PRId64, port);
    struct addrinfo hints;
    struct addrinfo *res = NULL;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_protocol = IPPROTO_TCP;
    hints.ai_flags = AI_PASSIVE;
    int rc = getaddrinfo(hostptr, portbuf, &hints, &res);
    if (rc != 0 || res == NULL) {
        return bloom_net_err(bloom_net_msg("tcp_listen", "cannot resolve bind address"));
    }
    bloom_net_sock_t sock = BLOOM_NET_INVALID_SOCK;
    for (struct addrinfo *ai = res; ai != NULL; ai = ai->ai_next) {
        sock = socket(ai->ai_family, ai->ai_socktype, ai->ai_protocol);
        if (sock == BLOOM_NET_INVALID_SOCK) {
            continue;
        }
        if (bind(sock, ai->ai_addr, (int)ai->ai_addrlen) == 0
            && listen(sock, (int)backlog) == 0) {
            break;
        }
        bloom_net_close_sock(sock);
        sock = BLOOM_NET_INVALID_SOCK;
    }
    freeaddrinfo(res);
    if (sock == BLOOM_NET_INVALID_SOCK) {
        return bloom_net_err(bloom_net_msg("tcp_listen", "bind or listen failed"));
    }
    int64_t handle = bloom_net_register(BLOOM_NET_KIND_LISTENER, sock);
    if (handle < 0) {
        bloom_net_close_sock(sock);
        return bloom_net_err(bloom_net_msg("tcp_listen", "no free handle slots"));
    }
    return bloom_net_ok(handle);
}

bloom_enum_Result_i64_str bloom_net_tcp_listen(
    bloom_str host,
    int64_t port,
    int64_t backlog
) {
    return bloom_net_listen_impl(host, port, backlog);
}

bloom_enum_Result_i64_str bloom_net_tcp_accept(int64_t listener) {
    bloom_net_entry *e = bloom_net_lookup(listener, BLOOM_NET_KIND_LISTENER);
    if (e == NULL) {
        return bloom_net_err(bloom_net_handle_err("tcp_accept", listener));
    }
    bloom_net_sock_t conn = accept(e->sock, NULL, NULL);
    if (conn == BLOOM_NET_INVALID_SOCK) {
        if (bloom_net_last_was_timeout()) {
            return bloom_net_err(bloom_net_msg("tcp_accept", "timeout"));
        }
        return bloom_net_err(bloom_net_msg("tcp_accept", "accept failed"));
    }
    int64_t handle = bloom_net_register(BLOOM_NET_KIND_CONNECTION, conn);
    if (handle < 0) {
        bloom_net_close_sock(conn);
        return bloom_net_err(bloom_net_msg("tcp_accept", "no free handle slots"));
    }
    return bloom_net_ok(handle);
}

/* ---- reading and writing ---------------------------------------------- */

static bloom_enum_Result_Option_str_str bloom_net_ok_opt(bloom_str value, bool some) {
    bloom_enum_Result_Option_str_str r;
    r.tag = 0;
    r.data.m_Ok._0.tag = some ? 0 : 1;
    if (some) {
        r.data.m_Ok._0.data.m_Some._0 = value;
    }
    return r;
}

static bloom_enum_Result_Option_str_str bloom_net_err_opt(bloom_str m) {
    bloom_enum_Result_Option_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bool bloom_net_utf8_valid(const unsigned char *s, size_t n) {
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

bloom_enum_Result_Option_str_str bloom_net_tcp_read(int64_t connection, int64_t max_bytes) {
    bloom_net_entry *e = bloom_net_lookup(connection, BLOOM_NET_KIND_CONNECTION);
    if (e == NULL) {
        return bloom_net_err_opt(bloom_net_handle_err("tcp_read", connection));
    }
    if (max_bytes < 1 || max_bytes > BLOOM_NET_MAX_READ) {
        return bloom_net_err_opt(bloom_net_msg(
            "tcp_read",
            "max_bytes out of range (1..1048576)"
        ));
    }
    char *buf = (char *)malloc((size_t)max_bytes);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    int got = bloom_net_recv(e->sock, buf, (int)max_bytes);
    if (got == 0) {
        free(buf);
        bloom_str empty;
        empty.rc = 0;
        empty.data = NULL;
        empty.len = 0;
        return bloom_net_ok_opt(empty, false);
    }
    if (got < 0) {
        free(buf);
        if (bloom_net_last_was_timeout()) {
            return bloom_net_err_opt(bloom_net_msg("tcp_read", "timeout"));
        }
        return bloom_net_err_opt(bloom_net_msg("tcp_read", "receive failed"));
    }
    if (!bloom_net_utf8_valid((const unsigned char *)buf, (size_t)got)) {
        free(buf);
        return bloom_net_err_opt(bloom_net_msg(
            "tcp_read",
            "received data is not valid UTF-8 (binary protocols are not supported)"
        ));
    }
    bloom_str data = bloom_str_from_bytes(buf, (size_t)got);
    free(buf);
    return bloom_net_ok_opt(data, true);
}

bloom_enum_Result_i64_str bloom_net_tcp_write_all(int64_t connection, bloom_str data) {
    bloom_net_entry *e = bloom_net_lookup(connection, BLOOM_NET_KIND_CONNECTION);
    if (e == NULL) {
        return bloom_net_err(bloom_net_handle_err("tcp_write_all", connection));
    }
    size_t sent = 0;
    while (sent < data.len) {
        size_t remaining = data.len - sent;
        int chunk = remaining > 0x40000000u ? 0x40000000 : (int)remaining;
        int n = bloom_net_send(e->sock, data.data + sent, chunk);
        if (n <= 0) {
            if (bloom_net_last_was_timeout()) {
                return bloom_net_err(bloom_net_msg("tcp_write_all", "timeout"));
            }
            return bloom_net_err(bloom_net_msg("tcp_write_all", "send failed"));
        }
        sent += (size_t)n;
    }
    return bloom_net_ok((int64_t)sent);
}

bloom_enum_Result_bool_str bloom_net_tcp_close(int64_t handle) {
    if (handle <= 0) {
        return bloom_net_err_bool(bloom_net_handle_err("tcp_close", handle));
    }
    int64_t index = (handle & BLOOM_NET_INDEX_MASK) - 1;
    uint32_t gen = (uint32_t)((uint64_t)handle >> BLOOM_NET_INDEX_BITS);
    if (index < 0 || index >= BLOOM_NET_MAX_HANDLES) {
        return bloom_net_err_bool(bloom_net_handle_err("tcp_close", handle));
    }
    bloom_net_entry *e = &bloom_net_table[index];
    if (e->kind == BLOOM_NET_KIND_FREE || e->gen != gen
        || e->kind == BLOOM_NET_KIND_TLS) {
        return bloom_net_err_bool(bloom_net_handle_err("tcp_close", handle));
    }
    bloom_net_close_sock(e->sock);
    bloom_live_sockets--;
    e->kind = BLOOM_NET_KIND_FREE;
    e->gen = bloom_net_next_gen++;
    if (bloom_net_next_gen == 0) {
        bloom_net_next_gen = 1;
    }
    return bloom_net_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_net_tcp_set_timeout(int64_t handle, int64_t milliseconds) {
    if (milliseconds < 0) {
        return bloom_net_err_bool(bloom_net_msg(
            "tcp_set_timeout",
            "negative timeout (0 disables the timeout)"
        ));
    }
    bloom_net_entry *e = bloom_net_lookup(handle, BLOOM_NET_KIND_CONNECTION);
    if (e == NULL) {
        e = bloom_net_lookup(handle, BLOOM_NET_KIND_LISTENER);
    }
    if (e == NULL) {
        return bloom_net_err_bool(bloom_net_handle_err("tcp_set_timeout", handle));
    }
    if (!bloom_net_set_sock_timeout(e->sock, milliseconds)) {
        return bloom_net_err_bool(bloom_net_msg("tcp_set_timeout", "setsockopt failed"));
    }
    return bloom_net_ok_bool(true);
}

bloom_enum_Result_i64_str bloom_net_local_port(int64_t handle) {
    bloom_net_entry *e = bloom_net_lookup(handle, BLOOM_NET_KIND_LISTENER);
    if (e == NULL) {
        e = bloom_net_lookup(handle, BLOOM_NET_KIND_CONNECTION);
    }
    if (e == NULL) {
        return bloom_net_err(bloom_net_handle_err("local_port", handle));
    }
    struct sockaddr_storage addr;
    memset(&addr, 0, sizeof addr);
    int len = (int)sizeof addr;
    if (getsockname(e->sock, (struct sockaddr *)&addr, &len) != 0) {
        return bloom_net_err(bloom_net_msg("local_port", "getsockname failed"));
    }
    if (addr.ss_family == AF_INET) {
        struct sockaddr_in *a = (struct sockaddr_in *)&addr;
        return bloom_net_ok((int64_t)ntohs(a->sin_port));
    }
    if (addr.ss_family == AF_INET6) {
        struct sockaddr_in6 *a = (struct sockaddr_in6 *)&addr;
        return bloom_net_ok((int64_t)ntohs(a->sin6_port));
    }
    return bloom_net_err(bloom_net_msg("local_port", "unknown address family"));
}
