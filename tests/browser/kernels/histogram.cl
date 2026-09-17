kernel void histogram(global const unsigned int *data,
                      global unsigned int *bins,
                      local unsigned int *tile)
{
    unsigned int lid = get_local_id(0);
    unsigned int gid = get_global_id(0);

    tile[lid] = 0;
    barrier(CLK_LOCAL_MEM_FENCE);

    unsigned int value = data[gid] & 15u;
    tile[value] += 1u;

    barrier(CLK_LOCAL_MEM_FENCE);

    if (lid < 16u) {
        bins[lid] += tile[lid];
    }
}
