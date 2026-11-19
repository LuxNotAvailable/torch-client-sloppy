// Torch Vibrant: the texel cache's storage (bufferObject.0), as the compute
// passes write it: a count and three pads, then five tables of CACHE_SIZE
// words. `ckeys` and `cvalues` are the coarse table; a coarse slot's value
// holds its claimer's along-normal offset until deferred_b shades it.
// deferred.slang reads the same layout.

RWByteAddressBuffer cache() { return iris_rw_buffer(IRIS_BUFFER_0); } // read_write

static const uint CACHE_COUNT = 0;
static const uint CACHE_KEYS = 16;
static const uint CACHE_VALUES = CACHE_KEYS + 4 * CACHE_SIZE;
static const uint CACHE_WORK = CACHE_VALUES + 4 * CACHE_SIZE;
static const uint CACHE_CKEYS = CACHE_WORK + 4 * CACHE_SIZE;
static const uint CACHE_CVALUES = CACHE_CKEYS + 4 * CACHE_SIZE;

uint cache_load(uint table, uint i) {
    return cache().Load(table + 4 * i);
}

void cache_store(uint table, uint i, uint value) {
    cache().Store(table + 4 * i, value);
}

uint cache_atomic_load(uint table, uint i) {
    uint seen;
    cache().InterlockedOr(table + 4 * i, 0u, seen);
    return seen;
}

void cache_atomic_store(uint table, uint i, uint value) {
    uint before;
    cache().InterlockedExchange(table + 4 * i, value, before);
}

/// The value there before; it took `value` if that was `expected`.
uint cache_compare_exchange(uint table, uint i, uint expected, uint value) {
    uint before;
    cache().InterlockedCompareExchange(table + 4 * i, expected, value, before);
    return before;
}

uint cache_add_count(uint n) {
    uint before;
    cache().InterlockedAdd(CACHE_COUNT, n, before);
    return before;
}
