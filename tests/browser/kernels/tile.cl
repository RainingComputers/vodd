kernel void tile(global const int *in, global int *out, local int *scratch)
{
    int lid = get_local_id(0);
    int gid = get_global_id(0);

    scratch[lid] = in[gid];
    barrier(CLK_LOCAL_MEM_FENCE);

    out[gid] = scratch[lid];
}
