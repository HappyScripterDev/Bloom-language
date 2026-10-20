/* Bloom std::time runtime — wall clock, monotonic clock, sleep, UTC formatting.
 *
 * Only emitted when the program loads `std::time`. All functions are plain
 * C helpers; the compiler calls them directly (no ownership transfer: the
 * arguments are borrowed, the returned string is freshly allocated).
 */

#include <time.h>
#include <errno.h>

/* Unix time: seconds since 1970-01-01T00:00:00Z (wall clock). The system
 * clock may be adjusted at any time; use monotonic_millis for durations. */
int64_t bloom_time_unix_seconds(void) {
#ifdef _WIN32
    FILETIME ft;
    ULARGE_INTEGER u;
    GetSystemTimeAsFileTime(&ft);
    u.LowPart = ft.dwLowDateTime;
    u.HighPart = ft.dwHighDateTime;
    return (int64_t)((u.QuadPart - 116444736000000000ULL) / 10000000ULL);
#else
    return (int64_t)time(NULL);
#endif
}

/* Unix time in milliseconds (same underlying clock as unix_seconds). */
int64_t bloom_time_unix_millis(void) {
#ifdef _WIN32
    FILETIME ft;
    ULARGE_INTEGER u;
    GetSystemTimeAsFileTime(&ft);
    u.LowPart = ft.dwLowDateTime;
    u.HighPart = ft.dwHighDateTime;
    return (int64_t)((u.QuadPart - 116444736000000000ULL) / 10000ULL);
#else
    struct timespec ts;
    if (clock_gettime(CLOCK_REALTIME, &ts) != 0) {
        bloom_panicf(NULL, 0, "time: clock_gettime(CLOCK_REALTIME) failed");
    }
    return (int64_t)ts.tv_sec * 1000 + (int64_t)(ts.tv_nsec / 1000000);
#endif
}

/* Monotonic milliseconds since an unspecified epoch (process start on most
 * systems). Intended for measuring durations, not as calendar time. */
int64_t bloom_time_monotonic_millis(void) {
#ifdef _WIN32
    LARGE_INTEGER freq;
    LARGE_INTEGER count;
    if (!QueryPerformanceFrequency(&freq) || freq.QuadPart == 0) {
        bloom_panicf(NULL, 0, "time: no monotonic clock available");
    }
    QueryPerformanceCounter(&count);
    return (int64_t)((count.QuadPart * 1000) / freq.QuadPart);
#else
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        bloom_panicf(NULL, 0, "time: clock_gettime(CLOCK_MONOTONIC) failed");
    }
    return (int64_t)ts.tv_sec * 1000 + (int64_t)(ts.tv_nsec / 1000000);
#endif
}

/* Sleeps for the given number of milliseconds. Zero is a no-op; negative
 * durations are a runtime error. Values larger than the platform limit are
 * slept in chunks. The operating system may oversleep. */
void bloom_time_sleep_ms(int64_t ms) {
    if (ms < 0) {
        bloom_panicf(NULL, 0, "sleep_ms: negative duration (%" PRId64 ")", ms);
    }
    if (ms == 0) {
        return;
    }
#ifdef _WIN32
    while (ms > 0) {
        DWORD chunk = (ms > (int64_t)0x7FFFFFFF) ? 0x7FFFFFFFu : (DWORD)ms;
        Sleep(chunk);
        ms -= (int64_t)chunk;
    }
#else
    {
        struct timespec req;
        req.tv_sec = (time_t)(ms / 1000);
        req.tv_nsec = (long)((ms % 1000) * 1000000);
        while (nanosleep(&req, &req) != 0 && errno == EINTR) {
        }
    }
#endif
}

/* Formats a Unix timestamp as ISO-8601 UTC: `YYYY-MM-DDTHH:MM:SSZ`.
 * Deterministic and independent of the local time zone. Years outside
 * 0000..9999 are a runtime error. */
bloom_str bloom_time_format_utc(int64_t secs, const char *file, int64_t line) {
    int64_t days = secs / 86400;
    int64_t rem = secs % 86400;
    if (rem < 0) {
        rem += 86400;
        days -= 1;
    }
    /* Howard Hinnant's civil_from_days algorithm. */
    int64_t z = days + 719468;
    int64_t era = (z >= 0 ? z : z - 146096) / 146097;
    int64_t doe = z - era * 146097;
    int64_t yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    int64_t y = yoe + era * 400;
    int64_t doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    int64_t mp = (5 * doy + 2) / 153;
    int64_t d = doy - (153 * mp + 2) / 5 + 1;
    int64_t m = mp < 10 ? mp + 3 : mp - 9;
    if (m <= 2) {
        y += 1;
    }
    if (y < 0 || y > 9999) {
        bloom_panicf(
            file,
            line,
            "format_utc: date out of representable range (year %" PRId64 ")",
            y
        );
    }
    int64_t hh = rem / 3600;
    int64_t mm = (rem % 3600) / 60;
    int64_t ss = rem % 60;
    char buf[40];
    snprintf(
        buf,
        sizeof buf,
        "%04" PRId64 "-%02" PRId64 "-%02" PRId64 "T%02" PRId64 ":%02" PRId64 ":%02" PRId64 "Z",
        y,
        m,
        d,
        hh,
        mm,
        ss
    );
    return bloom_str_from_bytes(buf, strlen(buf));
}
