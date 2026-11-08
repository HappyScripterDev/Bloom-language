/* Bloom std::collections runtime - string lists, maps and sets.
 *
 * First safe variant: elements are Bloom strings. Generic element types are
 * not implemented; this keeps ownership explicit and leak-free.
 * Limits: 1,000,000 elements per collection, capacity capped at 1,048,576.
 * Map/set keys must be non-empty (1..1048576 bytes); values may be up to 1 MiB.
 */

#define BLOOM_COLLECTIONS_MAX_ELEMENTS 1000000
#define BLOOM_COLLECTIONS_MAX_CAP 1048576
#define BLOOM_COLLECTIONS_MAX_STRING (1024 * 1024)

typedef struct {
    int rc;
    bloom_str *items;
    size_t len;
    size_t cap;
} bloom_list;

typedef struct {
    bloom_str key;
    bloom_str value;
    unsigned char state; /* 0 empty, 1 used, 2 tombstone */
} bloom_map_entry;

typedef struct {
    int rc;
    bloom_map_entry *entries;
    size_t cap;
    size_t len;
    size_t used;
} bloom_map;

typedef struct {
    bloom_str key;
    unsigned char state;
} bloom_set_entry;

typedef struct {
    int rc;
    bloom_set_entry *entries;
    size_t cap;
    size_t len;
    size_t used;
} bloom_set;

static bloom_str bloom_collections_msg(const char *op, const char *detail) {
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

static bool bloom_collections_key_ok(bloom_str key) {
    return key.len > 0 && key.len <= BLOOM_COLLECTIONS_MAX_STRING
        && memchr(key.data, 0, key.len) == NULL;
}

static bool bloom_collections_value_ok(bloom_str value) {
    return value.len <= BLOOM_COLLECTIONS_MAX_STRING
        && memchr(value.data, 0, value.len) == NULL;
}

static bloom_str bloom_collections_empty(void) {
    return bloom_str_from_bytes("", 0);
}

static size_t bloom_collections_hash(bloom_str s) {
    uint32_t h = 2166136261u;
    for (size_t i = 0; i < s.len; i++) {
        h ^= (unsigned char)s.data[i];
        h *= 16777619u;
    }
    return (size_t)h;
}

static bool bloom_collections_streq(bloom_str a, bloom_str b) {
    return a.len == b.len
        && (a.len == 0 || memcmp(a.data, b.data, a.len) == 0);
}

/* ---- list ------------------------------------------------------------- */

bloom_list_handle bloom_collections_list_retain(bloom_list_handle h) {
    bloom_list *list = (bloom_list *)(intptr_t)h;
    if (list != NULL) {
        list->rc++;
    }
    return h;
}

void bloom_collections_list_release(bloom_list_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_list *list = (bloom_list *)(intptr_t)*h;
    *h = 0;
    if (list->rc > 0) {
        list->rc--;
    }
    if (list->rc != 0) {
        return;
    }
    for (size_t i = 0; i < list->len; i++) {
        bloom_str_drop(&list->items[i]);
    }
    free(list->items);
    free(list);
}

static bloom_struct_std_collections_List bloom_collections_wrap_list(
    bloom_list *list
) {
    bloom_struct_std_collections_List v;
    v.f_id = (bloom_list_handle)(intptr_t)list;
    return v;
}

bloom_struct_std_collections_List bloom_collections_list_new(void) {
    bloom_list *list = (bloom_list *)calloc(1, sizeof *list);
    if (list == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    list->rc = 1;
    return bloom_collections_wrap_list(list);
}

static bool bloom_list_grow(bloom_list *list) {
    if (list->len < list->cap) {
        return true;
    }
    if (list->cap >= BLOOM_COLLECTIONS_MAX_ELEMENTS) {
        return false;
    }
    size_t cap = list->cap ? list->cap * 2 : 8;
    if (cap > BLOOM_COLLECTIONS_MAX_ELEMENTS) {
        cap = BLOOM_COLLECTIONS_MAX_ELEMENTS;
    }
    bloom_str *items = (bloom_str *)realloc(list->items, cap * sizeof *items);
    if (items == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    list->items = items;
    list->cap = cap;
    return true;
}

static bloom_enum_Result_bool_str bloom_collections_ok_bool(bool value) {
    bloom_enum_Result_bool_str r;
    r.tag = 0;
    r.data.m_Ok._0 = value;
    return r;
}

static bloom_enum_Result_bool_str bloom_collections_err_bool(bloom_str m) {
    bloom_enum_Result_bool_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

static bloom_enum_Result_str_str bloom_collections_ok_str(bloom_str v) {
    bloom_enum_Result_str_str r;
    r.tag = 0;
    r.data.m_Ok._0 = v;
    return r;
}

static bloom_enum_Result_str_str bloom_collections_err_str(bloom_str m) {
    bloom_enum_Result_str_str r;
    r.tag = 1;
    r.data.m_Err._0 = m;
    return r;
}

bloom_enum_Result_bool_str bloom_collections_list_push(
    bloom_struct_std_collections_List handle,
    bloom_str value
) {
    bloom_list *list = (bloom_list *)(intptr_t)handle.f_id;
    if (list == NULL) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "list_push",
            "invalid list handle"
        ));
    }
    if (!bloom_collections_value_ok(value)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "list_push",
            "value exceeds 1048576 bytes or contains NUL bytes"
        ));
    }
    if (!bloom_list_grow(list)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "list_push",
            "list capacity limit reached"
        ));
    }
    list->items[list->len++] = bloom_str_clone(value);
    return bloom_collections_ok_bool(true);
}

bloom_enum_Result_str_str bloom_collections_list_get(
    bloom_struct_std_collections_List handle,
    int64_t index
) {
    bloom_list *list = (bloom_list *)(intptr_t)handle.f_id;
    if (list == NULL) {
        return bloom_collections_err_str(bloom_collections_msg(
            "list_get",
            "invalid list handle"
        ));
    }
    if (index < 0 || (size_t)index >= list->len) {
        return bloom_collections_err_str(bloom_collections_msg(
            "list_get",
            "index is out of range"
        ));
    }
    return bloom_collections_ok_str(bloom_str_clone(list->items[index]));
}

bloom_enum_Result_bool_str bloom_collections_list_set(
    bloom_struct_std_collections_List handle,
    int64_t index,
    bloom_str value
) {
    bloom_list *list = (bloom_list *)(intptr_t)handle.f_id;
    if (list == NULL) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "list_set",
            "invalid list handle"
        ));
    }
    if (index < 0 || (size_t)index >= list->len) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "list_set",
            "index is out of range"
        ));
    }
    if (!bloom_collections_value_ok(value)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "list_set",
            "value exceeds 1048576 bytes or contains NUL bytes"
        ));
    }
    bloom_str replacement = bloom_str_clone(value);
    bloom_str_drop(&list->items[index]);
    list->items[index] = replacement;
    return bloom_collections_ok_bool(true);
}

bloom_enum_Result_bool_str bloom_collections_list_insert(
    bloom_struct_std_collections_List handle,
    int64_t index,
    bloom_str value
) {
    bloom_list *list = (bloom_list *)(intptr_t)handle.f_id;
    if (list == NULL) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "list_insert",
            "invalid list handle"
        ));
    }
    if (index < 0 || (size_t)index > list->len) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "list_insert",
            "index is out of range"
        ));
    }
    if (!bloom_collections_value_ok(value)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "list_insert",
            "value exceeds 1048576 bytes or contains NUL bytes"
        ));
    }
    if (!bloom_list_grow(list)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "list_insert",
            "list capacity limit reached"
        ));
    }
    if ((size_t)index < list->len) {
        memmove(
            &list->items[index + 1],
            &list->items[index],
            (list->len - (size_t)index) * sizeof *list->items
        );
    }
    list->items[index] = bloom_str_clone(value);
    list->len++;
    return bloom_collections_ok_bool(true);
}

bloom_enum_Result_str_str bloom_collections_list_remove(
    bloom_struct_std_collections_List handle,
    int64_t index
) {
    bloom_list *list = (bloom_list *)(intptr_t)handle.f_id;
    if (list == NULL) {
        return bloom_collections_err_str(bloom_collections_msg(
            "list_remove",
            "invalid list handle"
        ));
    }
    if (index < 0 || (size_t)index >= list->len) {
        return bloom_collections_err_str(bloom_collections_msg(
            "list_remove",
            "index is out of range"
        ));
    }
    bloom_str removed = list->items[index];
    memmove(
        &list->items[index],
        &list->items[index + 1],
        (list->len - (size_t)index - 1) * sizeof *list->items
    );
    list->len--;
    return bloom_collections_ok_str(removed);
}

bool bloom_collections_list_clear(bloom_struct_std_collections_List handle) {
    bloom_list *list = (bloom_list *)(intptr_t)handle.f_id;
    if (list == NULL) {
        return false;
    }
    for (size_t i = 0; i < list->len; i++) {
        bloom_str_drop(&list->items[i]);
    }
    list->len = 0;
    return true;
}

int64_t bloom_collections_list_len(bloom_struct_std_collections_List handle) {
    bloom_list *list = (bloom_list *)(intptr_t)handle.f_id;
    return list == NULL ? 0 : (int64_t)list->len;
}

int64_t bloom_collections_list_capacity(
    bloom_struct_std_collections_List handle
) {
    bloom_list *list = (bloom_list *)(intptr_t)handle.f_id;
    return list == NULL ? 0 : (int64_t)list->cap;
}

bloom_arr_str bloom_collections_list_values(
    bloom_struct_std_collections_List handle
) {
    bloom_arr_str out = bloom_arr_str_new();
    bloom_list *list = (bloom_list *)(intptr_t)handle.f_id;
    if (list == NULL) {
        return out;
    }
    for (size_t i = 0; i < list->len; i++) {
        bloom_arr_str_push_owned(out, bloom_str_clone(list->items[i]));
    }
    return out;
}

/* ---- map -------------------------------------------------------------- */

bloom_map_handle bloom_collections_map_retain(bloom_map_handle h) {
    bloom_map *map = (bloom_map *)(intptr_t)h;
    if (map != NULL) {
        map->rc++;
    }
    return h;
}

static void bloom_map_free_entries(bloom_map *map) {
    if (map->entries == NULL) {
        return;
    }
    for (size_t i = 0; i < map->cap; i++) {
        if (map->entries[i].state == 1) {
            bloom_str_drop(&map->entries[i].key);
            bloom_str_drop(&map->entries[i].value);
        }
    }
    free(map->entries);
    map->entries = NULL;
    map->cap = 0;
    map->len = 0;
    map->used = 0;
}

void bloom_collections_map_release(bloom_map_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_map *map = (bloom_map *)(intptr_t)*h;
    *h = 0;
    if (map->rc > 0) {
        map->rc--;
    }
    if (map->rc != 0) {
        return;
    }
    bloom_map_free_entries(map);
    free(map);
}

static bloom_struct_std_collections_Map bloom_collections_wrap_map(
    bloom_map *map
) {
    bloom_struct_std_collections_Map v;
    v.f_id = (bloom_map_handle)(intptr_t)map;
    return v;
}

bloom_struct_std_collections_Map bloom_collections_map_new(void) {
    bloom_map *map = (bloom_map *)calloc(1, sizeof *map);
    if (map == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    map->rc = 1;
    map->cap = 8;
    map->entries = (bloom_map_entry *)calloc(map->cap, sizeof *map->entries);
    if (map->entries == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    return bloom_collections_wrap_map(map);
}

static size_t bloom_map_find(bloom_map *map, bloom_str key, bool *found) {
    *found = false;
    size_t mask = map->cap - 1;
    size_t start = bloom_collections_hash(key) & mask;
    for (size_t step = 0; step < map->cap; step++) {
        size_t i = (start + step) & mask;
        if (map->entries[i].state == 0) {
            return i;
        }
        if (map->entries[i].state == 1
            && bloom_collections_streq(map->entries[i].key, key)) {
            *found = true;
            return i;
        }
    }
    return start;
}

static bool bloom_map_rehash(bloom_map *map, size_t cap) {
    bloom_map_entry *old = map->entries;
    size_t old_cap = map->cap;
    bloom_map_entry *fresh = (bloom_map_entry *)calloc(cap, sizeof *fresh);
    if (fresh == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    map->entries = fresh;
    map->cap = cap;
    map->used = 0;
    for (size_t i = 0; i < old_cap; i++) {
        if (old[i].state == 1) {
            bool found = false;
            size_t slot = bloom_map_find(map, old[i].key, &found);
            map->entries[slot] = old[i];
            map->used++;
        }
    }
    free(old);
    return true;
}

static bool bloom_map_prepare(bloom_map *map) {
    if ((map->used + 1) * 10 >= map->cap * 7) {
        if (map->cap >= BLOOM_COLLECTIONS_MAX_CAP) {
            return false;
        }
        return bloom_map_rehash(map, map->cap * 2);
    }
    return true;
}

bloom_enum_Result_bool_str bloom_collections_map_insert(
    bloom_struct_std_collections_Map handle,
    bloom_str key,
    bloom_str value
) {
    bloom_map *map = (bloom_map *)(intptr_t)handle.f_id;
    if (map == NULL) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "map_insert",
            "invalid map handle"
        ));
    }
    if (!bloom_collections_key_ok(key)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "map_insert",
            "key must be 1..1048576 bytes and must not contain NUL"
        ));
    }
    if (!bloom_collections_value_ok(value)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "map_insert",
            "value exceeds 1048576 bytes or contains NUL"
        ));
    }
    if (map->len >= BLOOM_COLLECTIONS_MAX_ELEMENTS || !bloom_map_prepare(map)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "map_insert",
            "map capacity limit reached"
        ));
    }
    bool found = false;
    size_t slot = bloom_map_find(map, key, &found);
    if (found) {
        bloom_str replacement = bloom_str_clone(value);
        bloom_str_drop(&map->entries[slot].value);
        map->entries[slot].value = replacement;
        return bloom_collections_ok_bool(true);
    }
    if (map->entries[slot].state == 0) {
        map->used++;
    }
    map->entries[slot].state = 1;
    map->entries[slot].key = bloom_str_clone(key);
    map->entries[slot].value = bloom_str_clone(value);
    map->len++;
    return bloom_collections_ok_bool(false);
}

bloom_enum_Option_str bloom_collections_map_get(
    bloom_struct_std_collections_Map handle,
    bloom_str key
) {
    bloom_enum_Option_str r;
    bloom_map *map = (bloom_map *)(intptr_t)handle.f_id;
    if (map == NULL || !bloom_collections_key_ok(key)) {
        r.tag = 1;
        return r;
    }
    bool found = false;
    size_t slot = bloom_map_find(map, key, &found);
    if (!found) {
        r.tag = 1;
        return r;
    }
    r.tag = 0;
    r.data.m_Some._0 = bloom_str_clone(map->entries[slot].value);
    return r;
}

bool bloom_collections_map_has(
    bloom_struct_std_collections_Map handle,
    bloom_str key
) {
    bloom_map *map = (bloom_map *)(intptr_t)handle.f_id;
    if (map == NULL || !bloom_collections_key_ok(key)) {
        return false;
    }
    bool found = false;
    bloom_map_find(map, key, &found);
    return found;
}

bloom_enum_Result_bool_str bloom_collections_map_remove(
    bloom_struct_std_collections_Map handle,
    bloom_str key
) {
    bloom_map *map = (bloom_map *)(intptr_t)handle.f_id;
    if (map == NULL) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "map_remove",
            "invalid map handle"
        ));
    }
    if (!bloom_collections_key_ok(key)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "map_remove",
            "key must be 1..1048576 bytes and must not contain NUL"
        ));
    }
    bool found = false;
    size_t slot = bloom_map_find(map, key, &found);
    if (!found) {
        return bloom_collections_ok_bool(false);
    }
    bloom_str_drop(&map->entries[slot].key);
    bloom_str_drop(&map->entries[slot].value);
    map->entries[slot].state = 2;
    map->len--;
    return bloom_collections_ok_bool(true);
}

int64_t bloom_collections_map_len(bloom_struct_std_collections_Map handle) {
    bloom_map *map = (bloom_map *)(intptr_t)handle.f_id;
    return map == NULL ? 0 : (int64_t)map->len;
}

bool bloom_collections_map_clear(bloom_struct_std_collections_Map handle) {
    bloom_map *map = (bloom_map *)(intptr_t)handle.f_id;
    if (map == NULL) {
        return false;
    }
    bloom_map_free_entries(map);
    map->cap = 8;
    map->entries = (bloom_map_entry *)calloc(map->cap, sizeof *map->entries);
    if (map->entries == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    return true;
}

bloom_arr_str bloom_collections_map_keys(
    bloom_struct_std_collections_Map handle
) {
    bloom_arr_str out = bloom_arr_str_new();
    bloom_map *map = (bloom_map *)(intptr_t)handle.f_id;
    if (map == NULL) {
        return out;
    }
    for (size_t i = 0; i < map->cap; i++) {
        if (map->entries[i].state == 1) {
            bloom_arr_str_push_owned(out, bloom_str_clone(map->entries[i].key));
        }
    }
    return out;
}

bloom_arr_str bloom_collections_map_values(
    bloom_struct_std_collections_Map handle
) {
    bloom_arr_str out = bloom_arr_str_new();
    bloom_map *map = (bloom_map *)(intptr_t)handle.f_id;
    if (map == NULL) {
        return out;
    }
    for (size_t i = 0; i < map->cap; i++) {
        if (map->entries[i].state == 1) {
            bloom_arr_str_push_owned(out, bloom_str_clone(map->entries[i].value));
        }
    }
    return out;
}

/* ---- set -------------------------------------------------------------- */

bloom_set_handle bloom_collections_set_retain(bloom_set_handle h) {
    bloom_set *set = (bloom_set *)(intptr_t)h;
    if (set != NULL) {
        set->rc++;
    }
    return h;
}

void bloom_collections_set_release(bloom_set_handle *h) {
    if (h == NULL || *h == 0) {
        return;
    }
    bloom_set *set = (bloom_set *)(intptr_t)*h;
    *h = 0;
    if (set->rc > 0) {
        set->rc--;
    }
    if (set->rc != 0) {
        return;
    }
    if (set->entries != NULL) {
        for (size_t i = 0; i < set->cap; i++) {
            if (set->entries[i].state == 1) {
                bloom_str_drop(&set->entries[i].key);
            }
        }
        free(set->entries);
    }
    free(set);
}

static bloom_struct_std_collections_Set bloom_collections_wrap_set(
    bloom_set *set
) {
    bloom_struct_std_collections_Set v;
    v.f_id = (bloom_set_handle)(intptr_t)set;
    return v;
}

bloom_struct_std_collections_Set bloom_collections_set_new(void) {
    bloom_set *set = (bloom_set *)calloc(1, sizeof *set);
    if (set == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    set->rc = 1;
    set->cap = 8;
    set->entries = (bloom_set_entry *)calloc(set->cap, sizeof *set->entries);
    if (set->entries == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    return bloom_collections_wrap_set(set);
}

static size_t bloom_set_find(bloom_set *set, bloom_str key, bool *found) {
    *found = false;
    size_t mask = set->cap - 1;
    size_t start = bloom_collections_hash(key) & mask;
    for (size_t step = 0; step < set->cap; step++) {
        size_t i = (start + step) & mask;
        if (set->entries[i].state == 0) {
            return i;
        }
        if (set->entries[i].state == 1
            && bloom_collections_streq(set->entries[i].key, key)) {
            *found = true;
            return i;
        }
    }
    return start;
}

static bool bloom_set_rehash(bloom_set *set, size_t cap) {
    bloom_set_entry *old = set->entries;
    size_t old_cap = set->cap;
    bloom_set_entry *fresh = (bloom_set_entry *)calloc(cap, sizeof *fresh);
    if (fresh == NULL) {
        bloom_panicf(NULL, 0, "out of memory");
    }
    set->entries = fresh;
    set->cap = cap;
    set->used = 0;
    for (size_t i = 0; i < old_cap; i++) {
        if (old[i].state == 1) {
            bool found = false;
            size_t slot = bloom_set_find(set, old[i].key, &found);
            set->entries[slot] = old[i];
            set->used++;
        }
    }
    free(old);
    return true;
}

static bool bloom_set_prepare(bloom_set *set) {
    if ((set->used + 1) * 10 >= set->cap * 7) {
        if (set->cap >= BLOOM_COLLECTIONS_MAX_CAP) {
            return false;
        }
        return bloom_set_rehash(set, set->cap * 2);
    }
    return true;
}

bloom_enum_Result_bool_str bloom_collections_set_insert(
    bloom_struct_std_collections_Set handle,
    bloom_str value
) {
    bloom_set *set = (bloom_set *)(intptr_t)handle.f_id;
    if (set == NULL) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "set_insert",
            "invalid set handle"
        ));
    }
    if (!bloom_collections_key_ok(value)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "set_insert",
            "value must be 1..1048576 bytes and must not contain NUL"
        ));
    }
    if (set->len >= BLOOM_COLLECTIONS_MAX_ELEMENTS || !bloom_set_prepare(set)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "set_insert",
            "set capacity limit reached"
        ));
    }
    bool found = false;
    size_t slot = bloom_set_find(set, value, &found);
    if (found) {
        return bloom_collections_ok_bool(false);
    }
    if (set->entries[slot].state == 0) {
        set->used++;
    }
    set->entries[slot].state = 1;
    set->entries[slot].key = bloom_str_clone(value);
    set->len++;
    return bloom_collections_ok_bool(true);
}

bool bloom_collections_set_has(
    bloom_struct_std_collections_Set handle,
    bloom_str value
) {
    bloom_set *set = (bloom_set *)(intptr_t)handle.f_id;
    if (set == NULL || !bloom_collections_key_ok(value)) {
        return false;
    }
    bool found = false;
    bloom_set_find(set, value, &found);
    return found;
}

bloom_enum_Result_bool_str bloom_collections_set_remove(
    bloom_struct_std_collections_Set handle,
    bloom_str value
) {
    bloom_set *set = (bloom_set *)(intptr_t)handle.f_id;
    if (set == NULL) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "set_remove",
            "invalid set handle"
        ));
    }
    if (!bloom_collections_key_ok(value)) {
        return bloom_collections_err_bool(bloom_collections_msg(
            "set_remove",
            "value must be 1..1048576 bytes and must not contain NUL"
        ));
    }
    bool found = false;
    size_t slot = bloom_set_find(set, value, &found);
    if (!found) {
        return bloom_collections_ok_bool(false);
    }
    bloom_str_drop(&set->entries[slot].key);
    set->entries[slot].state = 2;
    set->len--;
    return bloom_collections_ok_bool(true);
}

int64_t bloom_collections_set_len(bloom_struct_std_collections_Set handle) {
    bloom_set *set = (bloom_set *)(intptr_t)handle.f_id;
    return set == NULL ? 0 : (int64_t)set->len;
}

bool bloom_collections_set_clear(bloom_struct_std_collections_Set handle) {
    bloom_set *set = (bloom_set *)(intptr_t)handle.f_id;
    if (set == NULL) {
        return false;
    }
    for (size_t i = 0; i < set->cap; i++) {
        if (set->entries[i].state == 1) {
            bloom_str_drop(&set->entries[i].key);
        }
        set->entries[i].state = 0;
        set->entries[i].key.data = NULL;
        set->entries[i].key.len = 0;
        set->entries[i].key.rc = 0;
    }
    set->len = 0;
    set->used = 0;
    return true;
}

bloom_arr_str bloom_collections_set_values(
    bloom_struct_std_collections_Set handle
) {
    bloom_arr_str out = bloom_arr_str_new();
    bloom_set *set = (bloom_set *)(intptr_t)handle.f_id;
    if (set == NULL) {
        return out;
    }
    for (size_t i = 0; i < set->cap; i++) {
        if (set->entries[i].state == 1) {
            bloom_arr_str_push_owned(out, bloom_str_clone(set->entries[i].key));
        }
    }
    return out;
}
