// Torch Vibrant: the texel cache's storage, as the compute passes write it.
// Its layout matches CacheRead in deferred.wgsl.

struct Cache {
    count: atomic<u32>,
    pad0: u32,
    pad1: u32,
    pad2: u32,
    keys: array<atomic<u32>, 1048576>,
    values: array<u32, 1048576>,
    work: array<u32, 1048576>,
}

@group(0) @binding(IRIS_BUFFER_0) var<storage, read_write> cache: Cache;
