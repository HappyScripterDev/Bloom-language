/* Bloom std::tls runtime — TLS client connections via Windows Schannel.
 *
 * Only emitted when the program loads `std::tls` (or `std::http`); the net
 * runtime is emitted before it (handle registry, sockets, Result helpers).
 *
 * Design notes:
 *   - No custom cryptography: Schannel performs the protocol and crypto.
 *   - Schannel is configured with SCH_CRED_MANUAL_CRED_VALIDATION; this
 *     runtime performs certificate validation explicitly so both trust
 *     modes are uniform and testable:
 *       system trust: CertGetCertificateChain (system stores) plus
 *                     CertVerifyCertificateChainPolicy(CERT_CHAIN_POLICY_SSL)
 *                     with the requested hostname (chain, dates, EKU,
 *                     hostname).
 *       custom CA:    chain must terminate at exactly the provided CA
 *                     certificate; every element must be time-valid; the
 *                     leaf must carry the serverAuth EKU (if an EKU is
 *                     present) and match the hostname.
 *   - Minimum protocol: TLS 1.2 (SCH_USE_STRONG_CRYPTO, no silent fallback
 *     to older versions). TLS 1.3 is enabled when the SDK/OS provide it.
 *   - The handshake is bounded by a timeout (default 15 s; see
 *     tls_connect_ex); read/write timeouts are set with tls_set_timeout.
 *   - A connection that ends without a close_notify is reported as an
 *     error (the message may be incomplete), not as a clean EOF.
 *
 * POSIX: no TLS backend is provided in this version; all entry points
 * return an error on non-Windows platforms.
 */

#ifdef _WIN32

#define SECURITY_WIN32
#include <security.h>
#include <sspi.h>
#include <winternl.h>
#define SCHANNEL_USE_BLACKLISTS
#include <schannel.h>

typedef struct {
    CredHandle cred;
    CtxtHandle ctx;
    bool cred_valid;
    bool ctx_valid;
    bool tls13;
    bool clean_eof;
    char *plain;
    size_t plain_len;
    size_t plain_off;
    char *cipher;
    size_t cipher_len;
    size_t cipher_cap;
    size_t max_message;
} bloom_tls_state;

static bloom_enum_Result_i64_str bloom_tls_ok(int64_t v) {
    return bloom_net_ok(v);
}

static bloom_enum_Result_i64_str bloom_tls_err(bloom_str m) {
    return bloom_net_err(m);
}

static bloom_enum_Result_bool_str bloom_tls_ok_bool(bool v) {
    return bloom_net_ok_bool(v);
}

static bloom_enum_Result_bool_str bloom_tls_err_bool(bloom_str m) {
    return bloom_net_err_bool(m);
}

static bloom_enum_Result_Option_str_str bloom_tls_ok_opt(bloom_str value, bool some) {
    return bloom_net_ok_opt(value, some);
}

static bloom_enum_Result_Option_str_str bloom_tls_err_opt(bloom_str m) {
    return bloom_net_err_opt(m);
}

/* ---- error mapping ----------------------------------------------------- */

static bloom_str bloom_tls_sec_err(const char *op, SECURITY_STATUS sc) {
    const char *reason = "TLS error";
    char num[64];
    switch ((unsigned long)sc) {
        case SEC_E_UNTRUSTED_ROOT:
            reason = "certificate chain is not trusted";
            break;
        case SEC_E_WRONG_PRINCIPAL:
            reason = "certificate does not match the host name";
            break;
        case SEC_E_CERT_EXPIRED:
            reason = "certificate has expired";
            break;
        case SEC_E_ILLEGAL_MESSAGE:
            reason = "the peer did not speak TLS (protocol error)";
            break;
        case SEC_E_INCOMPLETE_CREDENTIALS:
            reason = "incomplete TLS credentials";
            break;
        case SEC_E_NO_CREDENTIALS:
            reason = "no TLS credentials available";
            break;
        case SEC_E_ALGORITHM_MISMATCH:
            reason = "no common TLS algorithm";
            break;
        default:
            snprintf(num, sizeof num, "TLS error 0x%08lx", (unsigned long)sc);
            reason = num;
            break;
    }
    return bloom_net_msg(op, reason);
}

static bloom_str bloom_tls_win_err(const char *op, DWORD code) {
    char buf[96];
    snprintf(buf, sizeof buf, "Windows error %lu", (unsigned long)code);
    return bloom_net_msg(op, buf);
}

/* ---- UTF-8 <-> UTF-16 -------------------------------------------------- */

static wchar_t *bloom_tls_to_utf16(const char *p, size_t n) {
    int need = MultiByteToWideChar(CP_UTF8, 0, p, (int)n, NULL, 0);
    if (need <= 0) {
        return NULL;
    }
    wchar_t *w = (wchar_t *)malloc(((size_t)need + 1) * sizeof(wchar_t));
    if (w == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    MultiByteToWideChar(CP_UTF8, 0, p, (int)n, w, need);
    w[need] = L'\0';
    return w;
}

/* ---- hostname matching (custom-CA mode) -------------------------------- */

static bool bloom_tls_label_eq(const char *a, size_t an, const char *b, size_t bn) {
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

/* RFC-6125-style match: case-insensitive; a leading `*.` wildcard matches
 * exactly one label. */
static bool bloom_tls_hostname_matches(const char *host, size_t host_len, const char *pattern) {
    size_t plen = strlen(pattern);
    if (plen == 0 || host_len == 0) {
        return false;
    }
    if (plen > 2 && pattern[0] == '*' && pattern[1] == '.') {
        const char *rest = pattern + 2;
        size_t rest_len = plen - 2;
        const char *dot = NULL;
        for (size_t i = 0; i < host_len; i++) {
            if (host[i] == '.') {
                dot = host + i;
                break;
            }
        }
        if (dot == NULL) {
            return false;
        }
        size_t first_label = (size_t)(dot - host);
        if (first_label == 0) {
            return false;
        }
        return bloom_tls_label_eq(dot + 1, host_len - first_label - 1, rest, rest_len);
    }
    return bloom_tls_label_eq(host, host_len, pattern, plen);
}

/* ---- certificate validation -------------------------------------------- */

static bloom_str bloom_tls_cert_error(const char *op, DWORD code) {
    const char *reason = NULL;
    char num[96];
    switch (code) {
        case CERT_E_UNTRUSTEDROOT:
            reason = "certificate is not trusted (untrusted root)";
            break;
        case CERT_E_UNTRUSTEDCA:
            reason = "certificate authority is not trusted";
            break;
        case CERT_E_CN_NO_MATCH:
            reason = "certificate host name mismatch";
            break;
        case CERT_E_EXPIRED:
            reason = "certificate has expired";
            break;
        case CERT_E_REVOKED:
            reason = "certificate has been revoked";
            break;
        case CERT_E_WRONG_USAGE:
            reason = "certificate is not valid for server authentication";
            break;
        case TRUST_E_CERT_SIGNATURE:
            reason = "certificate signature is invalid";
            break;
        default:
            snprintf(num, sizeof num, "certificate error 0x%08lx", (unsigned long)code);
            reason = num;
            break;
    }
    return bloom_net_msg(op, reason);
}

static bool bloom_tls_check_eku(PCCERT_CONTEXT cert) {
    CERT_ENHKEY_USAGE *usage = NULL;
    DWORD size = 0;
    if (!CertGetEnhancedKeyUsage(cert, 0, NULL, &size)) {
        /* No EKU information: treated as "any purpose" by Windows. */
        return true;
    }
    usage = (CERT_ENHKEY_USAGE *)malloc(size);
    if (usage == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    bool ok = false;
    if (CertGetEnhancedKeyUsage(cert, 0, usage, &size)) {
        if (usage->cUsageIdentifier == 0) {
            ok = true;
        }
        for (DWORD i = 0; i < usage->cUsageIdentifier && !ok; i++) {
            if (strcmp(usage->rgpszUsageIdentifier[i], szOID_PKIX_KP_SERVER_AUTH) == 0) {
                ok = true;
            }
        }
    }
    free(usage);
    return ok;
}

/* Matches the host against the certificate: SAN dNSName entries first
 * (including `*.` wildcards), SAN iPAddress entries for IP-literal hosts,
 * and finally the subject common name when no dNSName/iPAddress entry
 * exists (RFC 6125 behaviour). */
static bool bloom_tls_hostname_from_cert(
    PCCERT_CONTEXT cert,
    const char *host,
    size_t host_len
) {
    unsigned char ip[16];
    int ip_len = 0;
    if (host_len < 64) {
        char tmp[64];
        memcpy(tmp, host, host_len);
        tmp[host_len] = '\0';
        if (inet_pton(AF_INET, tmp, ip) == 1) {
            ip_len = 4;
        } else if (inet_pton(AF_INET6, tmp, ip) == 1) {
            ip_len = 16;
        }
    }
    bool any_san = false;
    PCERT_EXTENSION ext = CertFindExtension(
        szOID_SUBJECT_ALT_NAME2,
        cert->pCertInfo->cExtension,
        cert->pCertInfo->rgExtension
    );
    if (ext != NULL) {
        CERT_ALT_NAME_INFO *san = NULL;
        DWORD san_len = 0;
        if (CryptDecodeObjectEx(
                X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                X509_ALTERNATE_NAME,
                ext->Value.pbData,
                ext->Value.cbData,
                CRYPT_DECODE_ALLOC_FLAG,
                NULL,
                &san,
                &san_len)) {
            for (DWORD i = 0; i < san->cAltEntry; i++) {
                CERT_ALT_NAME_ENTRY *e = &san->rgAltEntry[i];
                if (e->dwAltNameChoice == CERT_ALT_NAME_DNS_NAME && e->pwszDNSName != NULL) {
                    any_san = true;
                    char narrow[512];
                    int m = WideCharToMultiByte(
                        CP_UTF8,
                        0,
                        e->pwszDNSName,
                        -1,
                        narrow,
                        (int)sizeof narrow,
                        NULL,
                        NULL
                    );
                    if (m > 1 && bloom_tls_hostname_matches(host, host_len, narrow)) {
                        LocalFree(san);
                        return true;
                    }
                } else if (e->dwAltNameChoice == CERT_ALT_NAME_IP_ADDRESS && ip_len > 0) {
                    any_san = true;
                    if ((int)e->IPAddress.cbData == ip_len
                        && memcmp(e->IPAddress.pbData, ip, (size_t)ip_len) == 0) {
                        LocalFree(san);
                        return true;
                    }
                }
            }
            LocalFree(san);
        }
    }
    if (any_san) {
        return false;
    }
    /* No SAN names: fall back to the common name. */
    char buf[512];
    DWORD n = CertGetNameStringA(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, NULL, buf, sizeof buf);
    return n > 1 && bloom_tls_hostname_matches(host, host_len, buf);
}

/* Validates the peer certificate. `ca_ctx` is the explicit trust anchor
 * (custom-CA mode) or NULL (system trust store). */
static bloom_str bloom_tls_validate_cert(
    PCCERT_CONTEXT leaf,
    const char *host,
    size_t host_len,
    PCCERT_CONTEXT ca_ctx
) {
    HCERTSTORE add_store = NULL;
    if (ca_ctx != NULL) {
        add_store = CertOpenStore(CERT_STORE_PROV_MEMORY, 0, 0, 0, NULL);
        if (add_store == NULL) {
            return bloom_tls_win_err("tls", GetLastError());
        }
        if (!CertAddCertificateContextToStore(add_store, ca_ctx, CERT_STORE_ADD_ALWAYS, NULL)) {
            DWORD e = GetLastError();
            CertCloseStore(add_store, 0);
            return bloom_tls_win_err("tls", e);
        }
    }
    CERT_CHAIN_PARA chain_para;
    memset(&chain_para, 0, sizeof chain_para);
    chain_para.cbSize = sizeof chain_para;
    PCCERT_CHAIN_CONTEXT chain = NULL;
    if (!CertGetCertificateChain(
            NULL,
            leaf,
            NULL,
            add_store,
            &chain_para,
            0,
            NULL,
            &chain)) {
        DWORD e = GetLastError();
        if (add_store != NULL) {
            CertCloseStore(add_store, 0);
        }
        return bloom_tls_win_err("tls", e);
    }
    if (add_store != NULL) {
        CertCloseStore(add_store, 0);
    }
    if (chain == NULL) {
        return bloom_net_msg("tls", "certificate chain could not be built");
    }
    bloom_str error;
    error.rc = 0;
    error.data = NULL;
    error.len = 0;
    if (ca_ctx == NULL) {
        /* System trust: full SSL policy including host name. */
        wchar_t *host_w = bloom_tls_to_utf16(host, host_len);
        if (host_w == NULL) {
            CertFreeCertificateChain(chain);
            return bloom_net_msg("tls", "invalid UTF-8 in host name");
        }
        SSL_EXTRA_CERT_CHAIN_POLICY_PARA ssl_para;
        memset(&ssl_para, 0, sizeof ssl_para);
        ssl_para.cbSize = sizeof ssl_para;
        ssl_para.dwAuthType = AUTHTYPE_SERVER;
        ssl_para.fdwChecks = 0;
        ssl_para.pwszServerName = host_w;
        CERT_CHAIN_POLICY_PARA policy_para;
        memset(&policy_para, 0, sizeof policy_para);
        policy_para.cbSize = sizeof policy_para;
        policy_para.pvExtraPolicyPara = &ssl_para;
        CERT_CHAIN_POLICY_STATUS status;
        memset(&status, 0, sizeof status);
        status.cbSize = sizeof status;
        if (!CertVerifyCertificateChainPolicy(
                CERT_CHAIN_POLICY_SSL,
                chain,
                &policy_para,
                &status)) {
            DWORD e = GetLastError();
            error = bloom_tls_win_err("tls", e);
        } else if (status.dwError != 0) {
            error = bloom_tls_cert_error("tls", status.dwError);
        }
        free(host_w);
    } else {
        /* Custom CA: the chain must end at exactly the provided anchor. */
        if (chain->cChain == 0 || chain->rgpChain[0]->cElement == 0) {
            error = bloom_net_msg("tls", "certificate chain is empty");
        } else {
            CERT_SIMPLE_CHAIN *simple = chain->rgpChain[0];
            PCCERT_CONTEXT root = simple->rgpElement[simple->cElement - 1]->pCertContext;
            if (!CertCompareCertificate(
                    X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                    root->pCertInfo,
                    ca_ctx->pCertInfo)) {
                error = bloom_net_msg(
                    "tls",
                    "certificate chain does not terminate at the provided CA"
                );
            }
            for (DWORD i = 0; i < simple->cElement && error.data == NULL; i++) {
                LONG vt = CertVerifyTimeValidity(
                    NULL,
                    simple->rgpElement[i]->pCertContext->pCertInfo
                );
                if (vt != 0) {
                    error = bloom_net_msg("tls", "certificate is expired or not yet valid");
                }
            }
            if (error.data == NULL && !bloom_tls_check_eku(leaf)) {
                error = bloom_net_msg(
                    "tls",
                    "certificate is not valid for server authentication"
                );
            }
            if (error.data == NULL && !bloom_tls_hostname_from_cert(leaf, host, host_len)) {
                error = bloom_net_msg("tls", "certificate host name mismatch");
            }
        }
    }
    CertFreeCertificateChain(chain);
    return error;
}

/* ---- CA file loading --------------------------------------------------- */

static bool bloom_tls_read_file(const char *path, size_t path_len, char **out, size_t *out_len) {
    wchar_t *w = bloom_tls_to_utf16(path, path_len);
    if (w == NULL) {
        return false;
    }
    FILE *f = _wfopen(w, L"rb");
    free(w);
    if (f == NULL) {
        return false;
    }
    if (fseek(f, 0, SEEK_END) != 0) {
        fclose(f);
        return false;
    }
    long size = ftell(f);
    if (size < 0 || size > 16 * 1024 * 1024) {
        fclose(f);
        return false;
    }
    if (fseek(f, 0, SEEK_SET) != 0) {
        fclose(f);
        return false;
    }
    char *buf = (char *)malloc((size_t)size + 1);
    if (buf == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    size_t got = fread(buf, 1, (size_t)size, f);
    fclose(f);
    if (got != (size_t)size) {
        free(buf);
        return false;
    }
    buf[got] = '\0';
    *out = buf;
    *out_len = got;
    return true;
}

/* Loads the first certificate from a PEM or DER file. */
static PCCERT_CONTEXT bloom_tls_load_ca(bloom_str ca_file, bloom_str *err) {
    char *data = NULL;
    size_t len = 0;
    if (!bloom_tls_read_file(ca_file.data, ca_file.len, &data, &len)) {
        *err = bloom_net_msg("tls", "CA file could not be read");
        return NULL;
    }
    PCCERT_CONTEXT cert = NULL;
    if (strstr(data, "-----BEGIN") != NULL) {
        DWORD der_len = 0;
        if (!CryptStringToBinaryA(data, (DWORD)len, CRYPT_STRING_BASE64HEADER, NULL, &der_len, NULL, NULL)) {
            free(data);
            *err = bloom_net_msg("tls", "CA file contains no valid PEM certificate");
            return NULL;
        }
        BYTE *der = (BYTE *)malloc(der_len);
        if (der == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        if (!CryptStringToBinaryA(data, (DWORD)len, CRYPT_STRING_BASE64HEADER, der, &der_len, NULL, NULL)) {
            free(der);
            free(data);
            *err = bloom_net_msg("tls", "CA file contains no valid PEM certificate");
            return NULL;
        }
        cert = CertCreateCertificateContext(X509_ASN_ENCODING | PKCS_7_ASN_ENCODING, der, der_len);
        free(der);
    } else {
        cert = CertCreateCertificateContext(
            X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
            (const BYTE *)data,
            (DWORD)len
        );
    }
    free(data);
    if (cert == NULL) {
        *err = bloom_net_msg("tls", "CA file is not a valid certificate (PEM or DER)");
    }
    return cert;
}

/* ---- session state ----------------------------------------------------- */

static void bloom_tls_free_state(bloom_tls_state *st) {
    if (st == NULL) {
        return;
    }
    if (st->ctx_valid) {
        DeleteSecurityContext(&st->ctx);
    }
    if (st->cred_valid) {
        FreeCredentialsHandle(&st->cred);
    }
    free(st->plain);
    free(st->cipher);
    free(st);
}

/* ---- handshake --------------------------------------------------------- */

/* Sends a buffer, respecting the socket send timeout. */
static bool bloom_tls_send_all(bloom_net_sock_t sock, const char *data, size_t len) {
    size_t sent = 0;
    while (sent < len) {
        int n = bloom_net_send(sock, data + sent, (int)(len - sent));
        if (n <= 0) {
            return false;
        }
        sent += (size_t)n;
    }
    return true;
}

/* Reads into the cipher buffer; returns 1 on data, 0 on clean EOF,
 * -1 on error/timeout. */
static int bloom_tls_recv_more(bloom_net_sock_t sock, bloom_tls_state *st) {
    if (st->cipher_len == st->cipher_cap) {
        size_t cap = st->cipher_cap ? st->cipher_cap * 2 : 16384;
        char *nd = (char *)realloc(st->cipher, cap);
        if (nd == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        st->cipher = nd;
        st->cipher_cap = cap;
    }
    int n = bloom_net_recv(sock, st->cipher + st->cipher_len, (int)(st->cipher_cap - st->cipher_len));
    if (n == 0) {
        return 0;
    }
    if (n < 0) {
        return -1;
    }
    st->cipher_len += (size_t)n;
    return 1;
}

static bloom_enum_Result_i64_str bloom_tls_connect_impl(
    bloom_str host,
    int64_t port,
    int64_t handshake_timeout_ms,
    bloom_str ca_file
) {
    if (port < 1 || port > 65535) {
        return bloom_tls_err(bloom_net_msg("tls_connect", "port out of range (1..65535)"));
    }
    if (handshake_timeout_ms < 1 || handshake_timeout_ms > 600000) {
        return bloom_tls_err(bloom_net_msg(
            "tls_connect",
            "handshake timeout out of range (1..600000 ms)"
        ));
    }
    char hostbuf[512];
    if (!bloom_net_host_to_buf(host, hostbuf, sizeof hostbuf)) {
        return bloom_tls_err(bloom_net_msg("tls_connect", "empty or too long host name"));
    }
    PCCERT_CONTEXT ca_ctx = NULL;
    bloom_str load_err;
    load_err.rc = 0;
    load_err.data = NULL;
    load_err.len = 0;
    if (ca_file.len > 0) {
        ca_ctx = bloom_tls_load_ca(ca_file, &load_err);
        if (ca_ctx == NULL) {
            return bloom_tls_err(load_err);
        }
    }
    if (!bloom_net_init()) {
        if (ca_ctx != NULL) {
            CertFreeCertificateContext(ca_ctx);
        }
        return bloom_tls_err(bloom_net_msg("tls_connect", "Winsock initialization failed"));
    }
    bloom_net_sock_t sock = BLOOM_NET_INVALID_SOCK;
    int cr = bloom_net_connect_raw(hostbuf, port, 0, &sock);
    if (cr == 1) {
        if (ca_ctx != NULL) {
            CertFreeCertificateContext(ca_ctx);
        }
        return bloom_tls_err(bloom_net_msg2("tls_connect", "cannot resolve host `", hostbuf));
    }
    if (cr != 0) {
        if (ca_ctx != NULL) {
            CertFreeCertificateContext(ca_ctx);
        }
        return bloom_tls_err(bloom_net_msg2("tls_connect", "connection failed to `", hostbuf));
    }
    /* Bound the handshake. */
    bloom_net_set_sock_timeout(sock, handshake_timeout_ms);

    bloom_tls_state *st = (bloom_tls_state *)calloc(1, sizeof(bloom_tls_state));
    if (st == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    SCH_CREDENTIALS creds;
    memset(&creds, 0, sizeof creds);
    creds.dwVersion = SCH_CREDENTIALS_VERSION;
    /* Require TLS 1.2 or newer: disable everything else. */
    TLS_PARAMETERS tls_params;
    memset(&tls_params, 0, sizeof tls_params);
    tls_params.grbitDisabledProtocols = ~(DWORD)(SP_PROT_TLS1_2_CLIENT
#ifdef SP_PROT_TLS1_3_CLIENT
        | SP_PROT_TLS1_3_CLIENT
#endif
    );
    creds.cTlsParameters = 1;
    creds.pTlsParameters = &tls_params;
    creds.dwFlags = SCH_CRED_NO_DEFAULT_CREDS | SCH_USE_STRONG_CRYPTO
        | SCH_CRED_MANUAL_CRED_VALIDATION;
    TimeStamp expiry;
    SECURITY_STATUS sc = AcquireCredentialsHandleW(
        NULL,
        UNISP_NAME_W,
        SECPKG_CRED_OUTBOUND,
        NULL,
        &creds,
        NULL,
        NULL,
        &st->cred,
        &expiry
    );
    if (sc != SEC_E_OK) {
        bloom_tls_free_state(st);
        bloom_net_close_sock(sock);
        if (ca_ctx != NULL) {
            CertFreeCertificateContext(ca_ctx);
        }
        return bloom_tls_err(bloom_tls_sec_err("tls_connect", sc));
    }
    st->cred_valid = true;

    wchar_t *host_w = bloom_tls_to_utf16(hostbuf, strlen(hostbuf));
    if (host_w == NULL) {
        bloom_tls_free_state(st);
        bloom_net_close_sock(sock);
        if (ca_ctx != NULL) {
            CertFreeCertificateContext(ca_ctx);
        }
        return bloom_tls_err(bloom_net_msg("tls_connect", "invalid UTF-8 in host name"));
    }

    bloom_str fail;
    fail.rc = 0;
    fail.data = NULL;
    fail.len = 0;
    bool handshake_ok = false;
    for (;;) {
        SecBuffer in_bufs[2];
        SecBufferDesc in_desc;
        in_bufs[0].BufferType = SECBUFFER_TOKEN;
        in_bufs[0].pvBuffer = st->cipher;
        in_bufs[0].cbBuffer = (unsigned long)st->cipher_len;
        in_bufs[1].BufferType = SECBUFFER_EMPTY;
        in_bufs[1].pvBuffer = NULL;
        in_bufs[1].cbBuffer = 0;
        in_desc.ulVersion = SECBUFFER_VERSION;
        in_desc.cBuffers = 2;
        in_desc.pBuffers = in_bufs;

        SecBuffer out_buf;
        SecBufferDesc out_desc;
        out_buf.BufferType = SECBUFFER_TOKEN;
        out_buf.pvBuffer = NULL;
        out_buf.cbBuffer = 0;
        out_desc.ulVersion = SECBUFFER_VERSION;
        out_desc.cBuffers = 1;
        out_desc.pBuffers = &out_buf;

        unsigned long attrs = 0;
        sc = InitializeSecurityContextW(
            &st->cred,
            st->ctx_valid ? &st->ctx : NULL,
            host_w,
            ISC_REQ_SEQUENCE_DETECT | ISC_REQ_REPLAY_DETECT | ISC_REQ_CONFIDENTIALITY
                | ISC_REQ_STREAM | ISC_REQ_ALLOCATE_MEMORY | ISC_RET_EXTENDED_ERROR,
            0,
            0,
            st->cipher_len > 0 ? &in_desc : NULL,
            0,
            &st->ctx,
            &out_desc,
            &attrs,
            &expiry
        );
        if (out_buf.pvBuffer != NULL) {
            if (!bloom_tls_send_all(sock, (const char *)out_buf.pvBuffer, out_buf.cbBuffer)) {
                if (out_buf.pvBuffer != NULL) {
                    FreeContextBuffer(out_buf.pvBuffer);
                    out_buf.pvBuffer = NULL;
                }
                fail = bloom_net_msg("tls_connect", "send failed during the TLS handshake");
                break;
            }
            FreeContextBuffer(out_buf.pvBuffer);
            out_buf.pvBuffer = NULL;
        }
        if (sc == SEC_I_CONTINUE_NEEDED || sc == SEC_E_INCOMPLETE_MESSAGE
            || sc == SEC_E_OK || sc == SEC_I_INCOMPLETE_CREDENTIALS) {
            /* The context exists after the first call, regardless of whether
             * the handshake is finished; later calls must reuse it. */
            st->ctx_valid = true;
        }
        if (sc == SEC_E_OK) {
            /* Keep any extra data that followed the handshake. */
            if (in_bufs[1].BufferType == SECBUFFER_EXTRA && in_bufs[1].cbBuffer > 0) {
                size_t extra = in_bufs[1].cbBuffer;
                memmove(st->cipher, st->cipher + (st->cipher_len - extra), extra);
                st->cipher_len = extra;
            } else {
                st->cipher_len = 0;
            }
            handshake_ok = true;
            break;
        }
        if (sc == SEC_I_CONTINUE_NEEDED || sc == SEC_E_INCOMPLETE_MESSAGE) {
            if (sc == SEC_I_CONTINUE_NEEDED && in_bufs[1].BufferType == SECBUFFER_EXTRA
                && in_bufs[1].cbBuffer > 0) {
                size_t extra = in_bufs[1].cbBuffer;
                memmove(st->cipher, st->cipher + (st->cipher_len - extra), extra);
                st->cipher_len = extra;
                continue;
            }
            if (sc == SEC_E_INCOMPLETE_MESSAGE) {
                /* Keep the partial data. */
            } else {
                st->cipher_len = 0;
            }
            int r = bloom_tls_recv_more(sock, st);
            if (r == 0) {
                fail = bloom_net_msg(
                    "tls_connect",
                    "connection closed during the TLS handshake"
                );
                break;
            }
            if (r < 0) {
                fail = bloom_net_last_was_timeout()
                    ? bloom_net_msg("tls_connect", "timeout during the TLS handshake")
                    : bloom_net_msg("tls_connect", "receive failed during the TLS handshake");
                break;
            }
            continue;
        }
        fail = bloom_tls_sec_err("tls_connect", sc);
        break;
    }
    free(host_w);
    if (!handshake_ok) {
        bloom_tls_free_state(st);
        bloom_net_close_sock(sock);
        if (ca_ctx != NULL) {
            CertFreeCertificateContext(ca_ctx);
        }
        return bloom_tls_err(fail);
    }

    /* Certificate validation. */
    PCCERT_CONTEXT remote = NULL;
    sc = QueryContextAttributes(&st->ctx, SECPKG_ATTR_REMOTE_CERT_CONTEXT, &remote);
    if (sc != SEC_E_OK || remote == NULL) {
        bloom_tls_free_state(st);
        bloom_net_close_sock(sock);
        if (ca_ctx != NULL) {
            CertFreeCertificateContext(ca_ctx);
        }
        return bloom_tls_err(bloom_net_msg(
            "tls_connect",
            "the server did not present a certificate"
        ));
    }
    bloom_str cert_err = bloom_tls_validate_cert(remote, hostbuf, strlen(hostbuf), ca_ctx);
    CertFreeCertificateContext(remote);
    if (ca_ctx != NULL) {
        CertFreeCertificateContext(ca_ctx);
    }
    if (cert_err.data != NULL) {
        bloom_tls_free_state(st);
        bloom_net_close_sock(sock);
        return bloom_tls_err(cert_err);
    }

    /* Stream sizes for read/write. */
    SecPkgContext_StreamSizes sizes;
    sc = QueryContextAttributes(&st->ctx, SECPKG_ATTR_STREAM_SIZES, &sizes);
    if (sc != SEC_E_OK) {
        bloom_tls_free_state(st);
        bloom_net_close_sock(sock);
        return bloom_tls_err(bloom_tls_sec_err("tls_connect", sc));
    }
    st->max_message = sizes.cbMaximumMessage;

    /* Remember whether TLS 1.3 was negotiated: its post-handshake messages
     * (NewSessionTicket, KeyUpdate) surface as SEC_I_RENEGOTIATE from
     * DecryptMessage and are processed by InitializeSecurityContext. */
    SecPkgContext_ConnectionInfo conn_info;
    memset(&conn_info, 0, sizeof conn_info);
    if (QueryContextAttributes(&st->ctx, SECPKG_ATTR_CONNECTION_INFO, &conn_info) == SEC_E_OK) {
#ifdef SP_PROT_TLS1_3_CLIENT
        st->tls13 = (conn_info.dwProtocol & SP_PROT_TLS1_3_CLIENT) != 0;
#else
        st->tls13 = false;
#endif
    }

    /* Clear the handshake timeout; read/write timeouts start unset. */
    bloom_net_set_sock_timeout(sock, 0);

    int64_t handle = bloom_net_register(BLOOM_NET_KIND_TLS, sock);
    if (handle < 0) {
        bloom_tls_free_state(st);
        bloom_net_close_sock(sock);
        return bloom_tls_err(bloom_net_msg("tls_connect", "no free handle slots"));
    }
    bloom_net_table[(handle & BLOOM_NET_INDEX_MASK) - 1].tls = st;
    return bloom_tls_ok(handle);
}

/* ---- public API -------------------------------------------------------- */

bloom_enum_Result_i64_str bloom_tls_connect(bloom_str host, int64_t port) {
    bloom_str empty;
    empty.rc = 0;
    empty.data = NULL;
    empty.len = 0;
    return bloom_tls_connect_impl(host, port, 15000, empty);
}

bloom_enum_Result_i64_str bloom_tls_connect_with_ca(
    bloom_str host,
    int64_t port,
    bloom_str ca_file
) {
    return bloom_tls_connect_impl(host, port, 15000, ca_file);
}

bloom_enum_Result_i64_str bloom_tls_connect_ex(
    bloom_str host,
    int64_t port,
    int64_t handshake_timeout_ms,
    bloom_str ca_file
) {
    return bloom_tls_connect_impl(host, port, handshake_timeout_ms, ca_file);
}

static bloom_net_entry *bloom_tls_lookup(int64_t handle, const char *op, bloom_str *err) {
    bloom_net_entry *e = bloom_net_lookup(handle, BLOOM_NET_KIND_TLS);
    if (e == NULL || e->tls == NULL) {
        *err = bloom_net_handle_err(op, handle);
        return NULL;
    }
    return e;
}

bloom_enum_Result_Option_str_str bloom_tls_read(int64_t handle, int64_t max_bytes) {
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    bloom_net_entry *e = bloom_tls_lookup(handle, "tls_read", &err);
    if (e == NULL) {
        return bloom_tls_err_opt(err);
    }
    bloom_tls_state *st = (bloom_tls_state *)e->tls;
    if (max_bytes < 1 || max_bytes > BLOOM_NET_MAX_READ) {
        return bloom_tls_err_opt(bloom_net_msg(
            "tls_read",
            "max_bytes out of range (1..1048576)"
        ));
    }
    if (st->clean_eof) {
        bloom_str empty;
        empty.rc = 0;
        empty.data = NULL;
        empty.len = 0;
        return bloom_tls_ok_opt(empty, false);
    }
    for (;;) {
        /* Serve already-decrypted data first. */
        if (st->plain_len > st->plain_off) {
            size_t avail = st->plain_len - st->plain_off;
            size_t take = avail < (size_t)max_bytes ? avail : (size_t)max_bytes;
            const char *src = st->plain + st->plain_off;
            if (!bloom_net_utf8_valid((const unsigned char *)src, take)) {
                return bloom_tls_err_opt(bloom_net_msg(
                    "tls_read",
                    "received data is not valid UTF-8 (binary protocols are not supported)"
                ));
            }
            bloom_str data = bloom_str_from_bytes(src, take);
            st->plain_off += take;
            if (st->plain_off == st->plain_len) {
                st->plain_off = 0;
                st->plain_len = 0;
            }
            return bloom_tls_ok_opt(data, true);
        }
        if (st->cipher_len == 0) {
            int r = bloom_tls_recv_more(e->sock, st);
            if (r == 0) {
                return bloom_tls_err_opt(bloom_net_msg(
                    "tls_read",
                    "connection ended without a TLS close_notify (message may be incomplete)"
                ));
            }
            if (r < 0) {
                return bloom_tls_err_opt(
                    bloom_net_last_was_timeout()
                        ? bloom_net_msg("tls_read", "timeout")
                        : bloom_net_msg("tls_read", "receive failed")
                );
            }
        }
        SecBuffer bufs[4];
        bufs[0].BufferType = SECBUFFER_DATA;
        bufs[0].pvBuffer = st->cipher;
        bufs[0].cbBuffer = (unsigned long)st->cipher_len;
        bufs[1].BufferType = SECBUFFER_EMPTY;
        bufs[2].BufferType = SECBUFFER_EMPTY;
        bufs[3].BufferType = SECBUFFER_EMPTY;
        SecBufferDesc desc;
        desc.ulVersion = SECBUFFER_VERSION;
        desc.cBuffers = 4;
        desc.pBuffers = bufs;
        unsigned long qop = 0;
        SECURITY_STATUS sc = DecryptMessage(&st->ctx, &desc, 0, &qop);
        if (sc == SEC_E_OK || sc == SEC_I_RENEGOTIATE) {
            char *data_ptr = NULL;
            size_t data_len = 0;
            size_t extra_len = 0;
            char *extra_ptr = NULL;
            for (int i = 0; i < 4; i++) {
                if (bufs[i].BufferType == SECBUFFER_DATA && bufs[i].cbBuffer > 0) {
                    data_ptr = (char *)bufs[i].pvBuffer;
                    data_len = bufs[i].cbBuffer;
                }
                if (bufs[i].BufferType == SECBUFFER_EXTRA && bufs[i].cbBuffer > 0) {
                    extra_ptr = (char *)bufs[i].pvBuffer;
                    extra_len = bufs[i].cbBuffer;
                }
            }
            if (sc == SEC_I_RENEGOTIATE) {
                if (!st->tls13) {
                    return bloom_tls_err_opt(bloom_net_msg(
                        "tls_read",
                        "TLS renegotiation is not supported"
                    ));
                }
                if (extra_len > 0 && extra_ptr != NULL) {
                    memmove(st->cipher, extra_ptr, extra_len);
                    st->cipher_len = extra_len;
                } else {
                    st->cipher_len = 0;
                }
                /* TLS 1.3 post-handshake messages (NewSessionTicket,
                 * KeyUpdate). They are processed with
                 * InitializeSecurityContext; a response token (KeyUpdate)
                 * is sent back, session tickets are consumed silently. */
                while (st->cipher_len > 0) {
                    SecBuffer hs_in[2];
                    SecBufferDesc hs_in_desc;
                    hs_in[0].BufferType = SECBUFFER_TOKEN;
                    hs_in[0].pvBuffer = st->cipher;
                    hs_in[0].cbBuffer = (unsigned long)st->cipher_len;
                    hs_in[1].BufferType = SECBUFFER_EMPTY;
                    hs_in[1].pvBuffer = NULL;
                    hs_in[1].cbBuffer = 0;
                    hs_in_desc.ulVersion = SECBUFFER_VERSION;
                    hs_in_desc.cBuffers = 2;
                    hs_in_desc.pBuffers = hs_in;
                    SecBuffer hs_out;
                    SecBufferDesc hs_out_desc;
                    hs_out.BufferType = SECBUFFER_TOKEN;
                    hs_out.pvBuffer = NULL;
                    hs_out.cbBuffer = 0;
                    hs_out_desc.ulVersion = SECBUFFER_VERSION;
                    hs_out_desc.cBuffers = 1;
                    hs_out_desc.pBuffers = &hs_out;
                    unsigned long hs_attrs = 0;
                    TimeStamp hs_expiry;
                    SECURITY_STATUS hs_sc = InitializeSecurityContextW(
                        &st->cred,
                        &st->ctx,
                        NULL,
                        ISC_REQ_SEQUENCE_DETECT | ISC_REQ_REPLAY_DETECT
                            | ISC_REQ_CONFIDENTIALITY | ISC_REQ_STREAM
                            | ISC_REQ_ALLOCATE_MEMORY | ISC_RET_EXTENDED_ERROR,
                        0,
                        0,
                        &hs_in_desc,
                        0,
                        &st->ctx,
                        &hs_out_desc,
                        &hs_attrs,
                        &hs_expiry
                    );
                    if (hs_out.pvBuffer != NULL) {
                        bool hs_sent = bloom_tls_send_all(
                            e->sock,
                            (const char *)hs_out.pvBuffer,
                            hs_out.cbBuffer
                        );
                        FreeContextBuffer(hs_out.pvBuffer);
                        if (!hs_sent) {
                            return bloom_tls_err_opt(bloom_net_msg("tls_read", "send failed"));
                        }
                    }
                    if (hs_in[1].BufferType == SECBUFFER_EXTRA && hs_in[1].cbBuffer > 0) {
                        size_t extra = hs_in[1].cbBuffer;
                        memmove(st->cipher, st->cipher + (st->cipher_len - extra), extra);
                        st->cipher_len = extra;
                    } else {
                        st->cipher_len = 0;
                    }
                    if (hs_sc == SEC_E_OK) {
                        break;
                    }
                    if (hs_sc != SEC_I_CONTINUE_NEEDED && hs_sc != SEC_E_INCOMPLETE_MESSAGE) {
                        return bloom_tls_err_opt(bloom_tls_sec_err("tls_read", hs_sc));
                    }
                    if (st->cipher_len == 0) {
                        int r = bloom_tls_recv_more(e->sock, st);
                        if (r == 0) {
                            return bloom_tls_err_opt(bloom_net_msg(
                                "tls_read",
                                "connection ended without a TLS close_notify (message may be incomplete)"
                            ));
                        }
                        if (r < 0) {
                            return bloom_tls_err_opt(
                                bloom_net_last_was_timeout()
                                    ? bloom_net_msg("tls_read", "timeout")
                                    : bloom_net_msg("tls_read", "receive failed")
                            );
                        }
                    }
                }
                continue;
            }
            /* Copy out the decrypted data before the extra bytes are moved
             * to the front of the buffer: the extra move can overlap the
             * data region. */
            bloom_str data;
            data.rc = 0;
            data.data = NULL;
            data.len = 0;
            bool have_data = false;
            if (data_len > 0) {
                if (!bloom_net_utf8_valid((const unsigned char *)data_ptr, data_len)) {
                    return bloom_tls_err_opt(bloom_net_msg(
                        "tls_read",
                        "received data is not valid UTF-8 (binary protocols are not supported)"
                    ));
                }
                size_t take = data_len < (size_t)max_bytes ? data_len : (size_t)max_bytes;
                data = bloom_str_from_bytes(data_ptr, take);
                if (take < data_len) {
                    size_t rest = data_len - take;
                    char *copy = (char *)malloc(rest);
                    if (copy == NULL) {
                        bloom_panicf(NULL, 0, "out of memory");
                    }
                    memcpy(copy, data_ptr + take, rest);
                    free(st->plain);
                    st->plain = copy;
                    st->plain_len = rest;
                    st->plain_off = 0;
                }
                have_data = true;
            }
            if (extra_len > 0 && extra_ptr != NULL) {
                memmove(st->cipher, extra_ptr, extra_len);
                st->cipher_len = extra_len;
            } else {
                st->cipher_len = 0;
            }
            if (have_data) {
                return bloom_tls_ok_opt(data, true);
            }
            continue;
        }
        if (sc == SEC_I_CONTEXT_EXPIRED) {
            /* Clean TLS shutdown by the peer. */
            st->clean_eof = true;
            st->cipher_len = 0;
            bloom_str empty;
            empty.rc = 0;
            empty.data = NULL;
            empty.len = 0;
            return bloom_tls_ok_opt(empty, false);
        }
        if (sc == SEC_E_INCOMPLETE_MESSAGE) {
            int r = bloom_tls_recv_more(e->sock, st);
            if (r == 0) {
                return bloom_tls_err_opt(bloom_net_msg(
                    "tls_read",
                    "connection ended without a TLS close_notify (message may be incomplete)"
                ));
            }
            if (r < 0) {
                return bloom_tls_err_opt(
                    bloom_net_last_was_timeout()
                        ? bloom_net_msg("tls_read", "timeout")
                        : bloom_net_msg("tls_read", "receive failed")
                );
            }
            continue;
        }
        return bloom_tls_err_opt(bloom_tls_sec_err("tls_read", sc));
    }
}

bloom_enum_Result_i64_str bloom_tls_write_all(int64_t handle, bloom_str data) {
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    bloom_net_entry *e = bloom_tls_lookup(handle, "tls_write_all", &err);
    if (e == NULL) {
        return bloom_tls_err(err);
    }
    bloom_tls_state *st = (bloom_tls_state *)e->tls;
    if (st->clean_eof) {
        return bloom_tls_err(bloom_net_msg(
            "tls_write_all",
            "the TLS session was closed by the peer"
        ));
    }
    size_t chunk_max = st->max_message > 0 ? st->max_message : 16384;
    size_t sent = 0;
    while (sent < data.len) {
        size_t chunk = data.len - sent;
        if (chunk > chunk_max) {
            chunk = chunk_max;
        }
        /* Layout: [stream header][data][stream trailer]. */
        size_t header_size = 5 + 16384; /* conservative upper bound */
        SecBuffer bufs[4];
        bufs[0].BufferType = SECBUFFER_STREAM_HEADER;
        bufs[0].pvBuffer = NULL;
        bufs[0].cbBuffer = 0;
        bufs[1].BufferType = SECBUFFER_DATA;
        bufs[1].pvBuffer = NULL;
        bufs[1].cbBuffer = 0;
        bufs[2].BufferType = SECBUFFER_STREAM_TRAILER;
        bufs[2].pvBuffer = NULL;
        bufs[2].cbBuffer = 0;
        bufs[3].BufferType = SECBUFFER_EMPTY;
        bufs[3].pvBuffer = NULL;
        bufs[3].cbBuffer = 0;
        /* Query exact sizes for this message. */
        SecPkgContext_StreamSizes sizes;
        SECURITY_STATUS sc = QueryContextAttributes(&st->ctx, SECPKG_ATTR_STREAM_SIZES, &sizes);
        if (sc != SEC_E_OK) {
            return bloom_tls_err(bloom_tls_sec_err("tls_write_all", sc));
        }
        header_size = sizes.cbHeader;
        size_t trailer_size = sizes.cbTrailer;
        char *block = (char *)malloc(header_size + chunk + trailer_size);
        if (block == NULL) {
            bloom_panicf(NULL, 0, "out of memory");
        }
        memcpy(block + header_size, data.data + sent, chunk);
        bufs[0].pvBuffer = block;
        bufs[0].cbBuffer = (unsigned long)header_size;
        bufs[1].pvBuffer = block + header_size;
        bufs[1].cbBuffer = (unsigned long)chunk;
        bufs[2].pvBuffer = block + header_size + chunk;
        bufs[2].cbBuffer = (unsigned long)trailer_size;
        SecBufferDesc desc;
        desc.ulVersion = SECBUFFER_VERSION;
        desc.cBuffers = 4;
        desc.pBuffers = bufs;
        sc = EncryptMessage(&st->ctx, 0, &desc, 0);
        if (sc != SEC_E_OK) {
            free(block);
            return bloom_tls_err(bloom_tls_sec_err("tls_write_all", sc));
        }
        size_t total = (size_t)bufs[0].cbBuffer + (size_t)bufs[1].cbBuffer
            + (size_t)bufs[2].cbBuffer;
        bool ok = bloom_tls_send_all(e->sock, block, total);
        free(block);
        if (!ok) {
            return bloom_tls_err(
                bloom_net_last_was_timeout()
                    ? bloom_net_msg("tls_write_all", "timeout")
                    : bloom_net_msg("tls_write_all", "send failed")
            );
        }
        sent += chunk;
    }
    return bloom_tls_ok((int64_t)sent);
}

bloom_enum_Result_bool_str bloom_tls_close(int64_t handle) {
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    bloom_net_entry *e = bloom_tls_lookup(handle, "tls_close", &err);
    if (e == NULL) {
        return bloom_tls_err_bool(err);
    }
    bloom_tls_state *st = (bloom_tls_state *)e->tls;
    /* Best-effort close_notify; bounded by a short send timeout. */
    if (st->ctx_valid && !st->clean_eof) {
        bloom_net_set_sock_timeout(e->sock, 1000);
        SecBuffer token;
        token.BufferType = SECBUFFER_TOKEN;
        token.pvBuffer = NULL;
        token.cbBuffer = 0;
        SecBufferDesc token_desc;
        token_desc.ulVersion = SECBUFFER_VERSION;
        token_desc.cBuffers = 1;
        token_desc.pBuffers = &token;
        if (ApplyControlToken(&st->ctx, &token_desc) == SEC_E_OK) {
            SecBuffer out_buf;
            out_buf.BufferType = SECBUFFER_TOKEN;
            out_buf.pvBuffer = NULL;
            out_buf.cbBuffer = 0;
            SecBufferDesc out_desc;
            out_desc.ulVersion = SECBUFFER_VERSION;
            out_desc.cBuffers = 1;
            out_desc.pBuffers = &out_buf;
            TimeStamp expiry;
            SECURITY_STATUS sc = InitializeSecurityContext(
                &st->cred,
                &st->ctx,
                NULL,
                ISC_REQ_STREAM,
                0,
                0,
                NULL,
                0,
                NULL,
                &out_desc,
                NULL,
                &expiry
            );
            if ((sc == SEC_E_OK || sc == SEC_I_CONTINUE_NEEDED) && out_buf.pvBuffer != NULL) {
                bloom_tls_send_all(e->sock, (const char *)out_buf.pvBuffer, out_buf.cbBuffer);
                FreeContextBuffer(out_buf.pvBuffer);
            }
        }
    }
    bloom_tls_free_state(st);
    e->tls = NULL;
    bloom_net_close_sock(e->sock);
    bloom_live_sockets--;
    e->kind = BLOOM_NET_KIND_FREE;
    e->gen = bloom_net_next_gen++;
    if (bloom_net_next_gen == 0) {
        bloom_net_next_gen = 1;
    }
    return bloom_tls_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_tls_set_timeout(int64_t handle, int64_t milliseconds) {
    if (milliseconds < 0) {
        return bloom_tls_err_bool(bloom_net_msg(
            "tls_set_timeout",
            "negative timeout (0 disables the timeout)"
        ));
    }
    bloom_str err;
    err.rc = 0;
    err.data = NULL;
    err.len = 0;
    bloom_net_entry *e = bloom_tls_lookup(handle, "tls_set_timeout", &err);
    if (e == NULL) {
        return bloom_tls_err_bool(err);
    }
    if (!bloom_net_set_sock_timeout(e->sock, milliseconds)) {
        return bloom_tls_err_bool(bloom_net_msg("tls_set_timeout", "setsockopt failed"));
    }
    return bloom_tls_ok_bool(true);
}

#else /* !_WIN32 */

/* No TLS backend is provided on this platform in this version. */

bloom_enum_Result_i64_str bloom_tls_connect(bloom_str host, int64_t port) {
    (void)host;
    (void)port;
    return bloom_net_err(bloom_net_msg("tls_connect", "TLS is only available on Windows in this version"));
}

bloom_enum_Result_i64_str bloom_tls_connect_with_ca(
    bloom_str host,
    int64_t port,
    bloom_str ca_file
) {
    (void)host;
    (void)port;
    (void)ca_file;
    return bloom_net_err(bloom_net_msg("tls_connect", "TLS is only available on Windows in this version"));
}

bloom_enum_Result_i64_str bloom_tls_connect_ex(
    bloom_str host,
    int64_t port,
    int64_t handshake_timeout_ms,
    bloom_str ca_file
) {
    (void)host;
    (void)port;
    (void)handshake_timeout_ms;
    (void)ca_file;
    return bloom_net_err(bloom_net_msg("tls_connect", "TLS is only available on Windows in this version"));
}

bloom_enum_Result_Option_str_str bloom_tls_read(int64_t handle, int64_t max_bytes) {
    (void)handle;
    (void)max_bytes;
    return bloom_net_err_opt(bloom_net_msg("tls_read", "TLS is only available on Windows in this version"));
}

bloom_enum_Result_i64_str bloom_tls_write_all(int64_t handle, bloom_str data) {
    (void)handle;
    (void)data;
    return bloom_net_err(bloom_net_msg("tls_write_all", "TLS is only available on Windows in this version"));
}

bloom_enum_Result_bool_str bloom_tls_close(int64_t handle) {
    (void)handle;
    return bloom_net_err_bool(bloom_net_msg("tls_close", "TLS is only available on Windows in this version"));
}

bloom_enum_Result_bool_str bloom_tls_set_timeout(int64_t handle, int64_t milliseconds) {
    (void)handle;
    (void)milliseconds;
    return bloom_net_err_bool(bloom_net_msg("tls_set_timeout", "TLS is only available on Windows in this version"));
}

#endif /* _WIN32 */
